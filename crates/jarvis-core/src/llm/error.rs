use std::fmt;

use crate::config;

// how far a request got before it failed. every variant carries the inputs
// that produced it - an error that does not name its endpoint is useless when
// there are two candidates (LM Studio :1234, ollama :11434).
#[derive(Debug, Clone)]
pub enum LlmError {
    // never left the process: a setting is empty, or refused by the
    // offline-first gate
    NotConfigured(String),

    // nothing is listening at the endpoint
    Connect { endpoint: String, source: String },

    // HTTP 401 - no token sent, or the one sent was rejected
    Unauthorized { endpoint: String, token_configured: bool, server_message: Option<String> },

    // HTTP 404, or a 400 whose body names the model
    ModelNotFound { endpoint: String, model: String, server_message: Option<String> },

    // any other non-2xx
    HttpStatus { endpoint: String, status: u16, server_message: Option<String> },

    // OUR tokio::time::timeout fired. never sniffed from reqwest: is_timeout()
    // and is_connect() can both be true for one error when connect_timeout
    // fires against an unreachable host
    Timeout { endpoint: String, secs: u64 },

    // 2xx, but the body is not an OpenAI chat completion
    Malformed { endpoint: String, detail: String, body_head: String },

    // the model hit the token ceiling before it produced any answer text.
    // reasoning models do this routinely: the scratchpad and the answer share
    // one budget, so a small ceiling buys thinking and no reply.
    Truncated { endpoint: String, max_tokens: u32, completion_tokens: u32 },

    // transport failure that is not a refused connect
    Transport { endpoint: String, source: String },
}

impl LlmError {
    // stable machine-readable discriminant. this is a WIRE CONTRACT with
    // IpcEvent::LlmAnswer.error_code, frontend/src/lib/ipc.ts and the
    // llm-error-* locale keys - renaming one means renaming all four.
    pub fn code(&self) -> &'static str {
        match self {
            LlmError::NotConfigured(_)     => "not_configured",
            LlmError::Connect { .. }       => "connect",
            LlmError::Unauthorized { .. }  => "unauthorized",
            LlmError::ModelNotFound { .. } => "model_not_found",
            LlmError::HttpStatus { .. }    => "http_status",
            LlmError::Timeout { .. }       => "timeout",
            LlmError::Truncated { .. }     => "truncated",
            LlmError::Malformed { .. }     => "malformed",
            LlmError::Transport { .. }     => "transport",
        }
    }
}

// "Server said: ..." tail, or nothing when the server gave us no body
fn said(msg: &Option<String>) -> String {
    match msg {
        Some(m) if !m.is_empty() => format!(" Server said: {}", m),
        _ => String::new(),
    }
}

// NOTHING here promises that a settings change applies by itself. it applies
// once the save reaches THIS process: jarvis-app reads app.db at startup and
// the settings window is a different process, so the GUI fires
// IpcAction::ReloadSettings after db_write_many and db::reload_llm_settings()
// adopts the new llm_* values. when that action cannot be delivered the GUI
// says a restart is needed (settings-saved-restart-hint) - which is exactly why
// these messages must not say the opposite.
impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LlmError::NotConfigured(msg) => write!(f, "LLM is not configured: {}", msg),

            LlmError::Connect { endpoint, source } => write!(f,
                "Cannot reach the LLM at {} - nothing is listening ({}). Start LM Studio's \
                 local server (Developer tab -> Start Server) or run `ollama serve`, then \
                 ask again. The address is the 'llm_base_url' setting key.",
                endpoint, source),

            LlmError::Unauthorized { endpoint, token_configured: false, server_message } => write!(f,
                "The LLM at {} requires an API token and none is configured. Copy the token \
                 from LM Studio's Developer tab into Settings -> LLM -> OpenAI Key (setting \
                 key 'api_key__openai') and save; the `lms` CLI can print the token too.{}",
                endpoint, said(server_message)),

            LlmError::Unauthorized { endpoint, token_configured: true, server_message } => write!(f,
                "The LLM at {} rejected the configured API token. Re-copy it from LM Studio's \
                 Developer tab into Settings -> LLM -> OpenAI Key (setting key \
                 'api_key__openai') and save.{}", endpoint, said(server_message)),

            LlmError::ModelNotFound { endpoint, model, server_message } => write!(f,
                "Model '{}' is not available at {}. Load it in LM Studio, or run \
                 `ollama pull {}`. The name is the 'llm_model' setting key and must match \
                 what the server reports exactly.{}",
                model, endpoint, model, said(server_message)),

            LlmError::HttpStatus { endpoint, status, server_message } => write!(f,
                "The LLM at {} returned HTTP {}. Nothing on this side can fix that - check \
                 the server's own log. The next utterance tries again.{}",
                endpoint, status, said(server_message)),

            // Three causes, in the order they bite. The reasoning one is the
            // last people guess and the easiest to hit: the model answers
            // perfectly well, spends a few hundred tokens thinking first, and
            // the turn is abandoned before a word of the answer exists. A
            // large llm_max_tokens next to a small llm_timeout is that trap
            // written into the settings - the budget says think as long as you
            // like, the clock says one minute.
            LlmError::Timeout { endpoint, secs } => write!(f,
                "The LLM at {} did not answer within {}s. Three things cause this: a model \
                 loading for the first time is far slower than a warm one, so preload it; a \
                 reasoning model spends its token budget thinking before it writes anything, \
                 so either turn thinking off or allow it the time; and a large \
                 'llm_max_tokens' beside a small 'llm_timeout' promises a long answer and \
                 then refuses to wait for it. The clock is 'llm_timeout' (seconds, {}-{}).",
                endpoint, secs, config::LLM_TIMEOUT_MIN, config::LLM_TIMEOUT_MAX),

            LlmError::Truncated { endpoint, max_tokens, completion_tokens } => write!(f,
                "The model at {} used its whole {} token budget ({} spent) before writing \
                 an answer, so the reply came back empty. Reasoning models put their \
                 thinking in the same budget - raise 'llm_max_tokens', or turn thinking \
                 off for this model in your server.",
                endpoint, max_tokens, completion_tokens),

            LlmError::Malformed { endpoint, detail, body_head } => write!(f,
                "The LLM at {} answered with something that is not an OpenAI chat completion \
                 ({}). Check that 'llm_base_url' points at an OpenAI-compatible /v1 root - \
                 LM Studio: http://127.0.0.1:1234/v1, ollama: http://127.0.0.1:11434/v1. \
                 Body starts: {}", endpoint, detail, body_head),

            LlmError::Transport { endpoint, source } => write!(f,
                "Request to the LLM at {} failed: {}. The next utterance tries again.",
                endpoint, source),
        }
    }
}

impl std::error::Error for LlmError {}
