use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use reqwest::Client;

use crate::config;
use crate::db::structs::is_loopback_url;
use crate::DB;

use super::error::LlmError;
use super::wire::{ChatMessage, ChatRequest, ChatResponse, ErrorEnvelope};

// not settings. a later stage can promote them; stage 1 keeps the surface small
const TEMPERATURE: f32 = 0.7;
// hard cap on one answer. an uncapped reasoning model can generate for minutes
// and blow the timeout on a question it already answered

// TCP connect alone. a REFUSED loopback connect takes ~2s on this box, so this
// must clear that comfortably or a live-but-busy server is misreported as down.
// the TOTAL budget is llm_timeout and is enforced by tokio::time::timeout in
// ask(); config::LLM_TIMEOUT_MIN is kept above this value so the total can
// never expire first and relabel "nothing is listening" as a timeout.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(6);

// LM Studio answers "Keep-Alive: timeout=5" while reqwest's pool default is
// 90s. voice turns are minutes apart, so a pooled socket is nearly always one
// the server already closed - "connection closed before message completed",
// intermittent, unreproducible, and matching none of the error variants above.
// one loopback TCP handshake per turn is cheaper than that bug.
const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(4);

// one client for the process lifetime.
//
// ClientBuilder::build() eagerly constructs the rustls ClientConfig including
// rustls_platform_verifier, which walks the Windows cert store - paid even for
// a plain http://127.0.0.1 target that never handshakes. lua/api/http.rs
// rebuilds per call; do NOT copy that here.
//
// nothing in this builder is per-config: base_url and the token travel on the
// request, so unlike commands/ahk.rs this cache never needs a key and never
// needs invalidating. changing either setting takes effect as soon as the live
// settings carry the new value - db::reload_llm_settings(), fired by the GUI
// over IpcAction::ReloadSettings after a save.
static CLIENT: Lazy<Result<Client, String>> = Lazy::new(|| {
    Client::builder()
        // reqwest enables the system proxy by default (auto_sys_proxy). on
        // Windows the bypass list is the registry's ProxyOverride verbatim, and
        // the literal "<local>" token Windows writes there is not understood by
        // hyper-util's matcher - so an enabled system proxy would route
        // 127.0.0.1 through it and fail inexplicably. this client is
        // loopback-first: no proxy, ever.
        .no_proxy()
        // reqwest defaults to Policy::limited(10). a single-hop call to a fixed
        // local endpoint has no legitimate redirect to follow, and following
        // one would walk straight around the loopback gate: a 307/308 from
        // whatever is listening on 127.0.0.1 replays the POST body - the
        // transcribed utterance - and the bearer token to an arbitrary host.
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .pool_idle_timeout(POOL_IDLE_TIMEOUT)
        .user_agent(concat!("jarvis/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())
});

// everything one turn needs, snapshotted out of the DB in one read
#[derive(Debug, Clone)]
pub struct LlmConfig {
    // no trailing slash
    pub base_url: String,
    pub model: String,
    // empty = send no Authorization header at all
    pub token: String,
    // empty = send no system message at all
    pub system_prompt: String,
    pub timeout_secs: u64,
    pub max_tokens: u32,
}

// One question and the answer it got, as the model will be shown it next time.
//
// Owned rather than borrowed: it outlives the turn that produced it by
// definition, and the whole point is to hand it to a later request.
#[derive(Debug, Clone, PartialEq)]
pub struct Exchange {
    pub user: String,
    pub assistant: String,
}

#[derive(Debug, Clone)]
pub struct LlmAnswer {
    pub text: String,
    // as the SERVER reported it, which is not always what was asked for
    pub model: String,
    pub elapsed_ms: u64,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

// is the no-command-found LLM turn switched on?
// deliberately separate from LlmConfig::from_settings so jarvis-cli can drive
// the client with the turn off - that harness is the whole point of the CLI path.
pub fn is_enabled() -> bool {
    DB.get().map(|db| db.read().llm_enabled).unwrap_or(false)
}

impl LlmConfig {
    pub fn from_settings() -> Result<LlmConfig, LlmError> {
        let (base_url, model, token, system_prompt, timeout_secs, max_tokens, thinking, allow_remote) = {
            let db = DB.get().ok_or_else(|| LlmError::NotConfigured(
                "settings are not initialized in this process".to_string()))?;
            let s = db.read();
            (s.llm_base_url.clone(), s.llm_model.clone(), s.api_keys.openai.clone(),
             s.llm_system_prompt.clone(), s.llm_timeout, s.llm_max_tokens,
             s.llm_thinking.clone(), s.llm_allow_remote)
        };
        let speaking = DB.get().map(|db| db.read().llm_speak).unwrap_or(false);

        let base_url = base_url.trim().trim_end_matches('/').to_string();
        if base_url.is_empty() {
            return Err(LlmError::NotConfigured(
                "'llm_base_url' is empty. LM Studio: http://127.0.0.1:1234/v1, \
                 ollama: http://127.0.0.1:11434/v1".to_string()));
        }

        // the offline-first gate, at the point of use. Settings::validate()
        // already refuses to SAVE a remote url with llm_allow_remote off, but
        // app.db can be hand-edited and Settings::default() never goes through
        // set(), so the promise is enforced again here - this is the check that
        // actually decides whether a packet leaves the machine.
        offline_gate(&base_url, allow_remote)?;

        let model = model.trim().to_string();
        if model.is_empty() {
            return Err(LlmError::NotConfigured(
                "'llm_model' is empty. Set it to the exact name the server reports - \
                 LM Studio: Developer tab, ollama: `ollama list`.".to_string()));
        }

        // clamped, not trusted: Settings::set enforces the range on the way in,
        // but Settings::default() and a hand-edited app.db never go through
        // set(), and a 0 here means every turn times out instantly
        let timeout_secs = timeout_secs.clamp(config::LLM_TIMEOUT_MIN, config::LLM_TIMEOUT_MAX);
        let max_tokens = max_tokens.clamp(config::LLM_MAX_TOKENS_MIN, config::LLM_MAX_TOKENS_MAX);

        // there is no request field for this - LM Studio's documented parameter
        // list has no reasoning switch - so the lever is the chat template's own
        // convention, read from the system prompt. Qwen3 honours it; a model
        // that does not simply reads one short line and ignores it.
        let system_prompt = if thinking == "off" {
            let base = system_prompt.trim();
            if base.is_empty() {
                config::LLM_NO_THINK_DIRECTIVE.to_string()
            } else {
                format!("{}
{}", base, config::LLM_NO_THINK_DIRECTIVE)
            }
        } else {
            system_prompt
        };

        // the answer is going to a speaker, so say so. Without this the model
        // is free to answer without punctuation, and unpunctuated text is read
        // as one flat run-on - the single biggest thing wrong with how a
        // spoken answer sounds.
        let system_prompt = if speaking {
            let base = system_prompt.trim();
            if base.is_empty() {
                config::LLM_SPEECH_STYLE_DIRECTIVE.to_string()
            } else {
                format!("{}
{}", base, config::LLM_SPEECH_STYLE_DIRECTIVE)
            }
        } else {
            system_prompt
        };

        Ok(LlmConfig { base_url, model, token: token.trim().to_string(),
                       system_prompt, timeout_secs, max_tokens })
    }
}

// one non-streaming chat completion.
//
// the TOTAL budget is enforced here with tokio::time::timeout rather than
// RequestBuilder::timeout or by sniffing reqwest: is_timeout() and is_connect()
// can both be true for the same error, so "we gave up" must be our own verdict
// and not a guess about which predicate to check first. dropping the reqwest
// future on timeout cancels the in-flight request.
pub async fn ask(
    cfg: &LlmConfig,
    prompt: &str,
    history: &[Exchange],
) -> Result<LlmAnswer, LlmError> {
    let started = Instant::now();
    let budget = Duration::from_secs(cfg.timeout_secs);

    match tokio::time::timeout(budget, send(cfg, prompt, history)).await {
        Ok(result) => result.map(|mut a| {
            a.elapsed_ms = started.elapsed().as_millis() as u64;
            a
        }),
        Err(_) => Err(LlmError::Timeout {
            endpoint: cfg.base_url.clone(),
            secs: cfg.timeout_secs,
        }),
    }
}

async fn send(
    cfg: &LlmConfig,
    prompt: &str,
    history: &[Exchange],
) -> Result<LlmAnswer, LlmError> {
    let client = CLIENT.as_ref().map_err(|e| LlmError::Transport {
        endpoint: cfg.base_url.clone(),
        source: format!("http client init failed: {}", e),
    })?;

    let url = format!("{}/chat/completions", cfg.base_url);

    let messages = build_messages(&cfg.system_prompt, history, prompt);

    let body = ChatRequest {
        model: cfg.model.as_str(),
        messages,
        stream: false,
        temperature: TEMPERATURE,
        max_tokens: cfg.max_tokens,
    };

    let mut req = client.post(&url).json(&body);
    // ollama ignores Authorization entirely, LM Studio requires it. one path
    // covers both; an empty token means we send no header and let LM Studio
    // tell us so, which is a clearer error than guessing.
    if !cfg.token.is_empty() {
        req = req.bearer_auth(&cfg.token);
    }

    let resp = req.send().await.map_err(|e| {
        // is_connect BEFORE is_timeout: connect_timeout against an unreachable
        // host sets both, and "nothing is listening" is the actionable one
        if e.is_connect() {
            LlmError::Connect { endpoint: cfg.base_url.clone(), source: e.to_string() }
        } else {
            LlmError::Transport { endpoint: cfg.base_url.clone(), source: e.to_string() }
        }
    })?;

    let status = resp.status();

    // NOT error_for_status(): it discards the body, and the body is the only
    // place LM Studio puts an actionable message
    let raw = resp.text().await.map_err(|e| LlmError::Transport {
        endpoint: cfg.base_url.clone(),
        source: format!("could not read the response body: {}", e),
    })?;

    if !status.is_success() {
        let server_message = serde_json::from_str::<ErrorEnvelope>(&raw)
            .ok()
            .map(|e| e.error.message().trim().to_string())
            .filter(|m| !m.is_empty())
            .or_else(|| {
                let head = head_of(&raw);
                if head.is_empty() { None } else { Some(head) }
            });

        return Err(match status.as_u16() {
            // LM Studio's auth middleware runs BEFORE routing - even GET /nope
            // 401s - so a 401 is unambiguously the token, never a wrong path
            // and never a wrong model
            401 => LlmError::Unauthorized {
                endpoint: cfg.base_url.clone(),
                token_configured: !cfg.token.is_empty(),
                server_message,
            },
            404 => LlmError::ModelNotFound {
                endpoint: cfg.base_url.clone(),
                model: cfg.model.clone(),
                server_message,
            },
            // some ollama builds answer 400 for an unknown model
            400 if server_message.as_deref()
                    .map(|m| m.to_lowercase().contains("model"))
                    .unwrap_or(false) => LlmError::ModelNotFound {
                endpoint: cfg.base_url.clone(),
                model: cfg.model.clone(),
                server_message,
            },
            code => LlmError::HttpStatus {
                endpoint: cfg.base_url.clone(),
                status: code,
                server_message,
            },
        });
    }

    // serde_json::from_str, not resp.json(): is_decode() alone tells the owner
    // nothing, and reading the text first lets the raw body into the message
    let parsed: ChatResponse = serde_json::from_str(&raw).map_err(|e| LlmError::Malformed {
        endpoint: cfg.base_url.clone(),
        detail: e.to_string(),
        body_head: head_of(&raw),
    })?;

    // choices CAN legitimately be empty, and content is null when a model
    // emitted only reasoning or a tool call. checked index, never [0].
    let (prompt_tokens, completion_tokens) = parsed.usage
        .as_ref()
        .map(|u| (u.prompt_tokens, u.completion_tokens))
        .unwrap_or((0, 0));

    let cut_off = parsed.choices.first()
        .and_then(|c| c.finish_reason.as_deref())
        .is_some_and(|r| r == "length");

    let text = parsed.choices.first()
        .and_then(|c| c.message.as_ref())
        .and_then(|m| m.content.as_deref())
        .map(strip_reasoning)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            // an empty answer that was cut off is a budget problem, not a
            // protocol one, and the remedy is completely different
            if cut_off {
                LlmError::Truncated {
                    endpoint: cfg.base_url.clone(),
                    max_tokens: cfg.max_tokens,
                    completion_tokens,
                }
            } else {
                LlmError::Malformed {
                    endpoint: cfg.base_url.clone(),
                    detail: "no usable choices[0].message.content in the response".to_string(),
                    body_head: head_of(&raw),
                }
            }
        })?
        .to_string();

    Ok(LlmAnswer {
        text,
        model: if parsed.model.is_empty() { cfg.model.clone() } else { parsed.model },
        elapsed_ms: 0, // ask() fills this in - it owns the clock
        prompt_tokens,
        completion_tokens,
    })
}

// reasoning models under LM Studio put their scratchpad inline in `content` as
// a leading <think>...</think> block instead of in `reasoning_content`. it is
// not the answer and it makes the GUI panel look broken. only a LEADING, CLOSED
// block is removed, and only when something is left after it - a body that
// merely mentions the tag is returned untouched.
fn strip_reasoning(text: &str) -> &str {
    if let Some(rest) = text.trim_start().strip_prefix("<think>") {
        if let Some(end) = rest.find("</think>") {
            let tail = rest[end + "</think>".len()..].trim();
            if !tail.is_empty() {
                return tail;
            }
        }
    }
    text
}

// first 200 CHARS of a body, for an error message.
// system prompt, then the conversation so far, then the question.
//
// An empty system prompt means NO system message, not an empty one: a few chat
// templates - the Gemma family among them - reject the system role outright
// and fail the whole request with a template error.
//
// A half exchange is dropped rather than sent. An assistant turn with no text
// teaches the model that answering with nothing is acceptable here, and a user
// turn with no answer after it reads as a question the assistant ignored.
// Neither should ever reach this point, which is exactly why it is worth being
// certain they cannot.
fn build_messages<'a>(
    system: &'a str,
    history: &'a [Exchange],
    prompt: &'a str,
) -> Vec<ChatMessage<'a>> {
    let mut messages = Vec::with_capacity(2 + history.len() * 2);

    if !system.trim().is_empty() {
        messages.push(ChatMessage { role: "system", content: system });
    }
    for turn in history {
        if turn.user.trim().is_empty() || turn.assistant.trim().is_empty() {
            continue;
        }
        messages.push(ChatMessage { role: "user", content: turn.user.as_str() });
        messages.push(ChatMessage { role: "assistant", content: turn.assistant.as_str() });
    }
    messages.push(ChatMessage { role: "user", content: prompt });
    messages
}

// char_indices, not &body[..200]: a byte slice through a Cyrillic answer panics.
fn head_of(body: &str) -> String {
    let cut = body.char_indices().nth(200).map(|(i, _)| i).unwrap_or(body.len());
    body[..cut].replace('\n', " ").trim().to_string()
}

// The offline-first gate, in one place.
//
// It is stated twice by design - Settings::validate() refuses to SAVE a remote
// url while llm_allow_remote is off, and this decides at the point of use - but
// the second statement must not drift from the first, and there are now two
// callers of it: a turn, and the settings screen asking a server what models it
// has. That screen asks with an address that has not been saved yet, which is
// exactly why the check cannot be left to the save path alone.
fn offline_gate(base_url: &str, allow_remote: bool) -> Result<(), LlmError> {
    if !is_loopback_url(base_url) && !allow_remote {
        return Err(LlmError::NotConfigured(format!(
            "'{}' is not a loopback address and 'llm_allow_remote' is off. jarvis is \
             offline-first: nothing leaves this machine until that setting is turned \
             on deliberately.", base_url)));
    }
    Ok(())
}

// ---------------------------------------------------------------- model list

// Ask the server which models it can serve.
//
// The settings screen used to take the model name as free text, so a typo and a
// server that had not loaded the model yet produced the same silence. Asking
// turns that into a choice.
//
// Called with what is TYPED in the form rather than what is stored, so the
// address can be tried before it is committed to - hence the explicit
// allow_remote argument instead of a read from the database.
pub async fn list_models(
    base_url: &str,
    token: &str,
    allow_remote: bool,
    timeout_secs: u64,
) -> Result<Vec<String>, LlmError> {
    let base_url = base_url.trim().trim_end_matches('/').to_string();
    if base_url.is_empty() {
        return Err(LlmError::NotConfigured(
            "the address is empty - there is nothing to ask.".to_string()));
    }
    offline_gate(&base_url, allow_remote)?;

    let endpoint = format!("{}/models", base_url);
    let client = CLIENT.as_ref().map_err(|e| LlmError::Transport {
        endpoint: endpoint.clone(),
        source: e.clone(),
    })?;

    let mut req = client.get(&endpoint);
    let token = token.trim();
    if !token.is_empty() {
        req = req.bearer_auth(token);
    }

    // same shape as a turn: OUR timeout decides, never reqwest's error flags -
    // is_timeout() and is_connect() can both be true for a single error.
    let resp = tokio::time::timeout(Duration::from_secs(timeout_secs), req.send())
        .await
        .map_err(|_| LlmError::Timeout { endpoint: endpoint.clone(), secs: timeout_secs })?
        .map_err(|e| {
            if e.is_connect() {
                LlmError::Connect { endpoint: endpoint.clone(), source: e.to_string() }
            } else {
                LlmError::Transport { endpoint: endpoint.clone(), source: e.to_string() }
            }
        })?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(LlmError::Unauthorized {
            endpoint,
            token_configured: !token.is_empty(),
            server_message: Some(head_of(&body)),
        });
    }
    if !status.is_success() {
        return Err(LlmError::HttpStatus {
            endpoint,
            status: status.as_u16(),
            server_message: Some(head_of(&body)),
        });
    }

    parse_models(&body, &endpoint)
}

// {"data":[{"id":"..."}]} - the OpenAI shape, which LM Studio and ollama's
// /v1/models both answer in. Split out from the request so the shapes that
// matter, and the ones that do not parse, can be tested without a server.
fn parse_models(body: &str, endpoint: &str) -> Result<Vec<String>, LlmError> {
    let parsed: serde_json::Value =
        serde_json::from_str(body).map_err(|e| LlmError::Malformed {
            endpoint: endpoint.to_string(),
            detail: format!("the model list is not JSON: {}", e),
            body_head: head_of(body),
        })?;

    let data = parsed
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| LlmError::Malformed {
            endpoint: endpoint.to_string(),
            detail: "the reply has no 'data' array - this does not look like an \
                     OpenAI-compatible /models".to_string(),
            body_head: head_of(body),
        })?;

    let mut ids: Vec<String> = data
        .iter()
        .filter_map(|m| m.get("id").and_then(|i| i.as_str()))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    // a server may list the same id twice across pages; the screen shows a list
    // of choices, and a duplicated choice is a bug the user has to look at
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_openai_shape() {
        let body = r#"{"object":"list","data":[
            {"id":"gemma-4-e4b-it-heretic","object":"model"},
            {"id":"qwen3-asr-1.7b","object":"model"}]}"#;
        assert_eq!(
            parse_models(body, "e").unwrap(),
            vec!["gemma-4-e4b-it-heretic".to_string(), "qwen3-asr-1.7b".to_string()]
        );
    }

    #[test]
    fn sorts_deduplicates_and_drops_blanks() {
        let body = r#"{"data":[{"id":"b"},{"id":"a"},{"id":"a"},{"id":"  "},{"id":" c "},{"no_id":1}]}"#;
        assert_eq!(
            parse_models(body, "e").unwrap(),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn an_empty_list_is_not_an_error() {
        // a server with nothing loaded answers {"data":[]}. that is a fact
        // about the server, not a failure - the screen says so and leaves the
        // name typeable rather than showing a red error.
        assert!(parse_models(r#"{"data":[]}"#, "e").unwrap().is_empty());
    }

    #[test]
    fn a_reply_without_data_is_malformed() {
        let err = parse_models(r#"{"models":["a"]}"#, "e").unwrap_err();
        assert_eq!(err.code(), "malformed");
    }

    #[test]
    fn a_reply_that_is_not_json_is_malformed() {
        // what a plain web server answers when the path is wrong: HTML
        let err = parse_models("<!doctype html><title>404</title>", "e").unwrap_err();
        assert_eq!(err.code(), "malformed");
    }

    fn exchange(u: &str, a: &str) -> Exchange {
        Exchange { user: u.to_string(), assistant: a.to_string() }
    }

    #[test]
    fn a_question_on_its_own_is_one_message() {
        let m = build_messages("", &[], "который час");
        assert_eq!(m.len(), 1);
        assert_eq!((m[0].role, m[0].content), ("user", "который час"));
    }

    #[test]
    fn an_empty_system_prompt_sends_no_system_message() {
        // not an empty one: the Gemma templates reject the system role and
        // fail the whole request rather than ignoring it
        assert_eq!(build_messages("   ", &[], "q").len(), 1);
        assert_eq!(build_messages("be brief", &[], "q")[0].role, "system");
    }

    #[test]
    fn the_conversation_goes_between_the_system_prompt_and_the_question() {
        let h = vec![exchange("какая погода", "ясно"), exchange("а завтра", "дождь")];
        let m = build_messages("be brief", &h, "а послезавтра");
        let shape: Vec<&str> = m.iter().map(|x| x.role).collect();
        assert_eq!(shape, vec!["system", "user", "assistant", "user", "assistant", "user"]);
        assert_eq!(m[1].content, "какая погода");
        assert_eq!(m.last().unwrap().content, "а послезавтра");
    }

    #[test]
    fn a_half_exchange_is_dropped() {
        // an assistant turn with no text teaches the model that answering with
        // nothing is fine here
        let h = vec![exchange("q", ""), exchange("", "a"), exchange("real", "answer")];
        let m = build_messages("", &h, "now");
        let shape: Vec<&str> = m.iter().map(|x| x.role).collect();
        assert_eq!(shape, vec!["user", "assistant", "user"]);
        assert_eq!(m[0].content, "real");
    }

    #[test]
    fn the_gate_refuses_a_remote_address_when_remote_is_off() {
        let err = offline_gate("http://192.168.1.50:1234/v1", false).unwrap_err();
        assert_eq!(err.code(), "not_configured");
    }

    #[test]
    fn the_gate_allows_loopback_with_remote_off() {
        assert!(offline_gate("http://127.0.0.1:1234/v1", false).is_ok());
    }

    #[test]
    fn the_gate_allows_a_remote_address_once_remote_is_on() {
        // the setting exists to be honoured, not to be a second lock
        assert!(offline_gate("http://192.168.1.50:1234/v1", true).is_ok());
    }
}
