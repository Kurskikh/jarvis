use crate::config;
use serde::{Deserialize, Serialize};

use crate::config::structs::SpeechToTextEngine;
use crate::config::structs::WakeWordEngine;
use crate::config::structs::NoiseSuppressionBackend;

// an app.db written before Porcupine was dropped still names it. serde would
// fail the whole struct, and db::init_settings() falls back to Settings::default()
// on ANY parse error - one stale value would silently wipe every other setting.
// so this field degrades on its own instead of taking the file down with it.
fn deserialize_wake_word_engine<'de, D>(d: D) -> Result<WakeWordEngine, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = String::deserialize(d)?;
    Ok(match raw.as_str() {
        "Rustpotter" => WakeWordEngine::Rustpotter,
        "Vosk" => WakeWordEngine::Vosk,
        other => {
            warn!("Unknown wake word engine '{}' in settings, using the default.", other);
            config::DEFAULT_WAKE_WORD_ENGINE
        }
    })
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Settings {
    pub microphone: i32,
    pub voice: String,

    #[serde(deserialize_with = "deserialize_wake_word_engine")]
    pub wake_word_engine: WakeWordEngine,

    // backend selections (string IDs matching model or code backend IDs)
    #[serde(default = "default_intent_backend")]
    pub intent_backend: String,
    #[serde(default = "default_slots_backend")]
    pub slots_backend: String,
    #[serde(default = "default_vad_backend")]
    pub vad_backend: String,

    pub gliner_model: String,

    pub speech_to_text_engine: SpeechToTextEngine,
    pub vosk_model: String,

    // audio processing
    pub noise_suppression: NoiseSuppressionBackend,
    pub gain_normalizer: bool,

    #[serde(default = "default_language")]
    pub language: String,

    // absolute path to an AutoHotkey interpreter (AutoHotkey.exe / AutoHotkeyUX.exe)
    // or to an AutoHotkey install directory. empty = discover via the registry.
    #[serde(default)]
    pub ahk_interpreter: String,

    // ### LLM (stage 1: text answer on no-command-found; no tools, no streaming)
    //
    // every field below carries its own serde default. Settings has no
    // container-level #[serde(default)] and db::init_settings() falls back to
    // Settings::default() on ANY parse error, so one undefaulted field added
    // here silently wipes an existing app.db - see the note at the top of this file.
    #[serde(default = "default_llm_enabled")]
    pub llm_enabled: bool,

    #[serde(default = "default_llm_base_url")]
    pub llm_base_url: String,

    #[serde(default)]
    pub llm_model: String,

    // SECONDS, not milliseconds. one unit end to end; the single conversion is
    // a Duration::from_secs in llm::ask().
    #[serde(default = "default_llm_timeout")]
    pub llm_timeout: u64,

    #[serde(default = "default_llm_max_tokens")]
    pub llm_max_tokens: u32,

    #[serde(default = "default_llm_thinking")]
    pub llm_thinking: String,

    #[serde(default = "default_llm_system_prompt")]
    pub llm_system_prompt: String,

    // the offline-first escape hatch. off = only loopback endpoints are
    // accepted. enforced in Settings::validate_change() at save time and again
    // in llm::LlmConfig::from_settings() at use time.
    #[serde(default = "default_llm_allow_remote")]
    pub llm_allow_remote: bool,

    // speak the answer, not just write it. Independent of llm_enabled on
    // purpose: turning the voice off while keeping answers is a reasonable
    // thing to want at two in the morning.
    #[serde(default = "default_llm_speak")]
    pub llm_speak: bool,

    // how closely the wake word has to match, in hundredths
    #[serde(default = "default_wake_min_score")]
    pub wake_min_score: u32,

    // how loud counts as speech, and how long a pause ends a phrase
    #[serde(default = "default_vad_energy_threshold")]
    pub vad_energy_threshold: u32,
    #[serde(default = "default_speech_pause_ms")]
    pub speech_pause_ms: u32,

    // quiet everything else while a turn is in progress?
    #[serde(default = "default_duck_others")]
    pub duck_others: bool,
    // what is LEFT, as a percentage of the volume the application had
    #[serde(default = "default_duck_level")]
    pub duck_level: u32,
    // how loud the assistant himself is, as a percentage of the recordings
    #[serde(default = "default_voice_volume")]
    pub voice_volume: u32,

    // does the assistant remember the conversation between questions?
    #[serde(default = "default_llm_history")]
    pub llm_history: bool,
    #[serde(default = "default_llm_history_turns")]
    pub llm_history_turns: u32,
    #[serde(default = "default_llm_history_idle_min")]
    pub llm_history_idle_min: u32,

    #[serde(default = "default_llm_tts_url")]
    pub llm_tts_url: String,

    #[serde(default = "default_llm_tts_mode")]
    pub llm_tts_mode: String,

    // interpreter to start the speech sidecar with. Empty = only connect to
    // one that is already running.
    #[serde(default = "default_llm_tts_python")]
    pub llm_tts_python: String,

    // the sidecar script itself. Kept separate from the interpreter rather
    // than derived from it: the two live wherever the owner installed
    // CosyVoice, and guessing one from the other would fail silently on any
    // layout but the one it was guessed against.
    #[serde(default)]
    pub llm_tts_script: String,

    // how the synthesiser should speak, not what it should say. See
    // config::DEFAULT_LLM_TTS_INSTRUCT for what was measured about it.
    #[serde(default)]
    pub llm_tts_instruct: String,

    // seconds to keep listening after the assistant finishes speaking, so the
    // next question needs no wake word. 0 turns it off.
    #[serde(default = "default_follow_up_secs")]
    pub follow_up_secs: u64,

    // seconds of silence that end a dialogue, once one has been started
    #[serde(default = "default_dialogue_exit_secs")]
    pub dialogue_exit_secs: u64,

    pub api_keys: ApiKeys,
}

fn default_intent_backend() -> String { config::DEFAULT_INTENT_BACKEND.to_string() }
fn default_slots_backend() -> String { config::DEFAULT_SLOTS_BACKEND.to_string() }
fn default_vad_backend() -> String { config::DEFAULT_VAD_BACKEND.to_string() }
fn default_language() -> String { crate::i18n::detect_system_language().to_string() }

fn default_llm_enabled() -> bool { config::DEFAULT_LLM_ENABLED }
fn default_llm_base_url() -> String { config::DEFAULT_LLM_BASE_URL.to_string() }
fn default_llm_timeout() -> u64 { config::DEFAULT_LLM_TIMEOUT }
fn default_llm_max_tokens() -> u32 { config::DEFAULT_LLM_MAX_TOKENS }
fn default_llm_thinking() -> String { config::DEFAULT_LLM_THINKING.to_string() }
fn default_llm_system_prompt() -> String { config::DEFAULT_LLM_SYSTEM_PROMPT.to_string() }
fn default_llm_allow_remote() -> bool { config::DEFAULT_LLM_ALLOW_REMOTE }
fn default_llm_speak() -> bool { config::DEFAULT_LLM_SPEAK }
fn default_wake_min_score() -> u32 { config::DEFAULT_WAKE_MIN_SCORE }
fn default_vad_energy_threshold() -> u32 { config::DEFAULT_VAD_ENERGY_THRESHOLD }
fn default_speech_pause_ms() -> u32 { config::DEFAULT_SPEECH_PAUSE_MS }
fn default_duck_others() -> bool { config::DEFAULT_DUCK_OTHERS }
fn default_duck_level() -> u32 { config::DEFAULT_DUCK_LEVEL }
fn default_voice_volume() -> u32 { config::DEFAULT_VOICE_VOLUME }
fn default_llm_history() -> bool { config::DEFAULT_LLM_HISTORY }
fn default_llm_history_turns() -> u32 { config::DEFAULT_LLM_HISTORY_TURNS }
fn default_llm_history_idle_min() -> u32 { config::DEFAULT_LLM_HISTORY_IDLE_MIN }
fn default_llm_tts_url() -> String { config::DEFAULT_LLM_TTS_URL.to_string() }
fn default_llm_tts_mode() -> String { config::DEFAULT_LLM_TTS_MODE.to_string() }
fn default_llm_tts_python() -> String { config::DEFAULT_LLM_TTS_PYTHON.to_string() }
fn default_follow_up_secs() -> u64 { config::DEFAULT_FOLLOW_UP_SECS }
fn default_dialogue_exit_secs() -> u64 { config::DEFAULT_DIALOGUE_EXIT_SECS }

// characters that must not appear in an endpoint url, because the WHATWG
// parser inside `url` - the one reqwest actually resolves with - reads them
// differently from any naive split:
//   '\'  is a PATH delimiter for http/https, so "http://evil.com\@127.0.0.1/v1"
//        has host evil.com there while a split on '/' '?' '#' plus "text after
//        the last @" sees 127.0.0.1 here (url-2.5.8 parser.rs:899 in
//        parse_userinfo and :1008 in parse_host). that is a straight bypass of
//        the loopback gate: the gate passes and the prompt leaves the machine.
//   tab/CR/LF are STRIPPED before parsing, so they can move the authority
//        boundary after this function has looked at it.
//   non-ASCII goes through IDNA and can map to a host that is not what it looks
//        like here.
// none of them has any business in a local endpoint address.
pub fn url_has_unsafe_char(url: &str) -> bool {
    url.bytes().any(|b| b == b'\\' || b == b' ' || b.is_ascii_control() || !b.is_ascii())
}

// is this url's host a loopback address? the offline-first gate.
//
// hand-rolled on purpose: adding the `url` crate to jarvis-core for six lines
// would land in jarvis-gui too, which builds with default-features = false and
// no optional deps at all. that means this parser must FAIL CLOSED wherever it
// could disagree with the real one - see url_has_unsafe_char.
pub fn is_loopback_url(url: &str) -> bool {
    if url_has_unsafe_char(url) {
        return false;
    }

    let after_scheme = match url.split_once("://") {
        Some((_, rest)) => rest,
        None => return false,
    };
    // strip path / query / fragment, then userinfo
    let authority = after_scheme.split(['/', '?', '#']).next().unwrap_or("");
    let hostport = match authority.rsplit_once('@') {
        Some((_, h)) => h,
        None => authority,
    };
    // [::1]:1234 -> ::1 ; 127.0.0.1:1234 -> 127.0.0.1
    let host = if let Some(rest) = hostport.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        hostport.split(':').next().unwrap_or("")
    };

    if host.eq_ignore_ascii_case("localhost") || host == "::1" {
        return true;
    }
    // the whole 127.0.0.0/8 block, and only when the host really parses as
    // IPv4 - "0.0.0.0" parses and is correctly NOT loopback
    matches!(host.parse::<std::net::Ipv4Addr>(), Ok(ip) if ip.is_loopback())
}

// validate a backend id against the model registry.
// when the registry is not initialized in this process (jarvis-cli) we cannot
// judge, so the value is accepted as-is; Settings::sanitize_backends() then
// clamps it in whichever process does have a registry.
fn validated_backend(
    task: crate::models::Task,
    key: &str,
    val: &str,
) -> Result<String, String> {
    match crate::models::check_backend(task, val) {
        Some(false) => {
            let valid: Vec<String> = crate::models::get_options(task)
                .into_iter()
                .map(|o| o.id)
                .collect();
            Err(format!(
                "invalid value for '{}': '{}' (valid: {})",
                key,
                val,
                valid.join(", ")
            ))
        }
        _ => Ok(val.to_string()),
    }
}

// reset one backend field to the task's default if the registry says its
// current value is unknown. plain fn, not a closure, to avoid closure-lifetime
// unification across the three call sites.
//
// only Some(false) resets. check_backend() returns None when this process
// cannot judge - no registry (jarvis-cli) or no models directory at all - and
// the reset is persisted, so a guess here permanently overwrites a choice the
// user made in a build that COULD see the models.
fn fix_backend(
    key: &'static str,
    task: crate::models::Task,
    field: &mut String,
    fixed: &mut Vec<(&'static str, String, String)>,
) {
    if crate::models::check_backend(task, field.as_str()) == Some(false) {
        let default = crate::models::default_backend(task);
        fixed.push((key, field.clone(), default.to_string()));
        *field = default.to_string();
    }
}

// ### KEY-VALUE ACCESS

impl Settings {
    /// read a setting by key. returns None for unknown keys.
    pub fn get(&self, key: &str) -> Option<String> {
        match key {
            "selected_microphone"       => Some(self.microphone.to_string()),
            "assistant_voice"           => Some(self.voice.clone()),
            "selected_wake_word_engine" => Some(format!("{:?}", self.wake_word_engine)),
            "intent_backend"            => Some(self.intent_backend.clone()),
            "slots_backend"             => Some(self.slots_backend.clone()),
            "vad_backend"               => Some(self.vad_backend.clone()),
            "selected_gliner_model"     => Some(self.gliner_model.clone()),
            "selected_vosk_model"       => Some(self.vosk_model.clone()),
            // the same spelling the backend list offers, not the Debug one:
            // a getter answering "TOne" against an option called "t-one" is a
            // picker that never matches its own value
            "speech_to_text_engine"     => Some(match self.speech_to_text_engine {
                SpeechToTextEngine::Vosk => "vosk".to_string(),
                SpeechToTextEngine::TOne => "t-one-ru".to_string(),
            }),
            "noise_suppression"         => Some(format!("{:?}", self.noise_suppression)),
            "gain_normalizer"           => Some(self.gain_normalizer.to_string()),
            "language"                  => Some(self.language.clone()),
            "ahk_interpreter"           => Some(self.ahk_interpreter.clone()),
            "llm_enabled"               => Some(self.llm_enabled.to_string()),
            "llm_base_url"              => Some(self.llm_base_url.clone()),
            "llm_model"                 => Some(self.llm_model.clone()),
            "llm_timeout"               => Some(self.llm_timeout.to_string()),
            "llm_max_tokens"            => Some(self.llm_max_tokens.to_string()),
            "llm_thinking"              => Some(self.llm_thinking.clone()),
            "llm_system_prompt"         => Some(self.llm_system_prompt.clone()),
            "llm_allow_remote"          => Some(self.llm_allow_remote.to_string()),
            "llm_speak"                 => Some(self.llm_speak.to_string()),
            "wake_min_score"            => Some(self.wake_min_score.to_string()),
            "vad_energy_threshold"      => Some(self.vad_energy_threshold.to_string()),
            "speech_pause_ms"           => Some(self.speech_pause_ms.to_string()),
            "duck_others"               => Some(self.duck_others.to_string()),
            "duck_level"                => Some(self.duck_level.to_string()),
            "voice_volume"              => Some(self.voice_volume.to_string()),
            "llm_history"               => Some(self.llm_history.to_string()),
            "llm_history_turns"         => Some(self.llm_history_turns.to_string()),
            "llm_history_idle_min"      => Some(self.llm_history_idle_min.to_string()),
            "llm_tts_url"               => Some(self.llm_tts_url.clone()),
            "llm_tts_mode"              => Some(self.llm_tts_mode.clone()),
            "llm_tts_python"            => Some(self.llm_tts_python.clone()),
            "llm_tts_script"            => Some(self.llm_tts_script.clone()),
            "llm_tts_instruct"          => Some(self.llm_tts_instruct.clone()),
            "follow_up_secs"            => Some(self.follow_up_secs.to_string()),
            "dialogue_exit_secs"        => Some(self.dialogue_exit_secs.to_string()),
            "api_key__openai"           => Some(self.api_keys.openai.clone()),
            _ => None,
        }
    }

    /// write a setting by key. returns Err for unknown keys or invalid values.
    pub fn set(&mut self, key: &str, val: &str) -> Result<(), String> {
        match key {
            "selected_microphone" => {
                self.microphone = val.parse::<i32>()
                    .map_err(|_| format!("invalid integer: '{}'", val))?;
            }
            "assistant_voice" => {
                self.voice = val.to_string();
            }
            "selected_wake_word_engine" => {
                self.wake_word_engine = match val.to_lowercase().as_str() {
                    "rustpotter" => WakeWordEngine::Rustpotter,
                    "vosk"       => WakeWordEngine::Vosk,
                    _ => return Err(format!("unknown wake word engine: '{}'", val)),
                };
            }
            "intent_backend" => {
                self.intent_backend =
                    validated_backend(crate::models::Task::Intent, key, val)?;
            }
            "slots_backend" => {
                self.slots_backend =
                    validated_backend(crate::models::Task::Slots, key, val)?;
            }
            "vad_backend" => {
                self.vad_backend =
                    validated_backend(crate::models::Task::Vad, key, val)?;
            }
            "selected_gliner_model" => {
                self.gliner_model = val.to_string();
            }
            "selected_vosk_model" => {
                self.vosk_model = val.to_string();
            }
            "noise_suppression" => {
                self.noise_suppression = match val.to_lowercase().as_str() {
                    "none"        => NoiseSuppressionBackend::None,
                    "nnnoiseless" => NoiseSuppressionBackend::Nnnoiseless,
                    _ => return Err(format!("unknown noise suppression backend: '{}'", val)),
                };
            }
            "gain_normalizer" => {
                self.gain_normalizer = match val.to_lowercase().as_str() {
                    "true"  => true,
                    "false" => false,
                    _ => return Err(format!("expected 'true' or 'false', got: '{}'", val)),
                };
            }
            "language" => {
                self.language = val.to_string();
            }
            "ahk_interpreter" => {
                let path = val.trim();
                if !path.is_empty() && !std::path::Path::new(path).exists() {
                    return Err(format!("path does not exist: '{}'", path));
                }
                self.ahk_interpreter = path.to_string();
            }
            "llm_enabled" => {
                self.llm_enabled = match val.to_lowercase().as_str() {
                    "true"  => true,
                    "false" => false,
                    _ => return Err(format!("expected 'true' or 'false', got: '{}'", val)),
                };
            }
            "llm_base_url" => {
                // SHAPE ONLY. the loopback gate is a cross-field rule and lives
                // in Settings::validate() - see the note there for why it
                // cannot be checked from inside set().
                let url = val.trim().trim_end_matches('/');
                if url.is_empty() {
                    return Err("base url must not be empty".to_string());
                }
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    return Err(format!("base url must start with http:// or https://: '{}'", val));
                }
                // rejected HERE, with a message, rather than silently failing
                // the loopback test later: a backslash or a stray control
                // character makes this url mean one thing to the gate and
                // another to reqwest (see url_has_unsafe_char)
                if url_has_unsafe_char(url) {
                    return Err(format!(
                        "base url contains a character that is not allowed in an endpoint \
                         address (backslash, space, control or non-ASCII): '{}'", val));
                }
                let host = url.split_once("://")
                    .map(|(_, r)| r.split(['/', '?', '#']).next().unwrap_or(""))
                    .unwrap_or("");
                if host.is_empty() {
                    return Err(format!("base url has no host: '{}'", val));
                }
                self.llm_base_url = url.to_string();
            }
            "llm_model" => {
                self.llm_model = val.trim().to_string();
            }
            "llm_timeout" => {
                let secs = val.parse::<u64>()
                    .map_err(|_| format!("invalid integer: '{}'", val))?;
                // the floor is above llm::client::CONNECT_TIMEOUT on purpose.
                // the total budget is enforced by tokio::time::timeout around
                // the whole call, so a budget SHORTER than the connect timeout
                // reports an unreachable endpoint as Timeout ("raise
                // llm_timeout") instead of Connect ("start the server") - the
                // wrong remedy for the commonest failure.
                if !(config::LLM_TIMEOUT_MIN..=config::LLM_TIMEOUT_MAX).contains(&secs) {
                    return Err(format!("timeout must be {}-{} seconds, got: '{}'",
                                       config::LLM_TIMEOUT_MIN, config::LLM_TIMEOUT_MAX, val));
                }
                self.llm_timeout = secs;
            }
            "llm_max_tokens" => {
                let n = val.parse::<u32>()
                    .map_err(|_| format!("invalid integer: '{}'", val))?;
                if !(config::LLM_MAX_TOKENS_MIN..=config::LLM_MAX_TOKENS_MAX).contains(&n) {
                    return Err(format!("max tokens must be {}-{}, got: '{}'",
                                       config::LLM_MAX_TOKENS_MIN, config::LLM_MAX_TOKENS_MAX, val));
                }
                self.llm_max_tokens = n;
            }
            "llm_thinking" => {
                match val {
                    "auto" | "off" => self.llm_thinking = val.to_string(),
                    _ => return Err(format!("thinking must be 'auto' or 'off', got: '{}'", val)),
                }
            }
            "llm_system_prompt" => {
                // stored verbatim: leading whitespace and newlines are the
                // author's business
                self.llm_system_prompt = val.to_string();
            }
            "llm_allow_remote" => {
                self.llm_allow_remote = match val.to_lowercase().as_str() {
                    "true"  => true,
                    "false" => false,
                    _ => return Err(format!("expected 'true' or 'false', got: '{}'", val)),
                };
            }
            "llm_speak" => {
                self.llm_speak = match val.to_lowercase().as_str() {
                    "true"  => true,
                    "false" => false,
                    _ => return Err(format!("expected 'true' or 'false', got: '{}'", val)),
                };
            }
            // The engine has had a getter and no setter since it was added, so
            // the window offered a choice that could not be made and the
            // running assistant read a constant instead of either. All three
            // now agree.
            "speech_to_text_engine" => {
                self.speech_to_text_engine = match val.to_lowercase().as_str() {
                    "vosk"  => SpeechToTextEngine::Vosk,
                    // "t-one-ru" is the id the descriptor carries and the
                    // one the dropdown offers; the shorter spellings are
                    // accepted so a hand-edited app.db is not a trap
                    "t-one-ru" | "t-one" | "tone" => SpeechToTextEngine::TOne,
                    _ => return Err(format!("unknown speech-to-text engine: '{}'", val)),
                };
            }
            "wake_min_score" => {
                let n = val.parse::<u32>()
                    .map_err(|_| format!("invalid integer: '{}'", val))?;
                if !(config::WAKE_MIN_SCORE_MIN..=config::WAKE_MIN_SCORE_MAX).contains(&n) {
                    return Err(format!("wake score must be {}-{}, got: '{}'",
                                       config::WAKE_MIN_SCORE_MIN,
                                       config::WAKE_MIN_SCORE_MAX, val));
                }
                self.wake_min_score = n;
            }
            "vad_energy_threshold" => {
                let n = val.parse::<u32>()
                    .map_err(|_| format!("invalid integer: '{}'", val))?;
                if !(config::VAD_ENERGY_THRESHOLD_MIN..=config::VAD_ENERGY_THRESHOLD_MAX).contains(&n) {
                    return Err(format!("loudness must be {}-{}, got: '{}'",
                                       config::VAD_ENERGY_THRESHOLD_MIN,
                                       config::VAD_ENERGY_THRESHOLD_MAX, val));
                }
                self.vad_energy_threshold = n;
            }
            "speech_pause_ms" => {
                let n = val.parse::<u32>()
                    .map_err(|_| format!("invalid integer: '{}'", val))?;
                if !(config::SPEECH_PAUSE_MS_MIN..=config::SPEECH_PAUSE_MS_MAX).contains(&n) {
                    return Err(format!("pause must be {}-{} ms, got: '{}'",
                                       config::SPEECH_PAUSE_MS_MIN,
                                       config::SPEECH_PAUSE_MS_MAX, val));
                }
                self.speech_pause_ms = n;
            }
            "duck_others" => {
                self.duck_others = match val.to_lowercase().as_str() {
                    "true"  => true,
                    "false" => false,
                    _ => return Err(format!("expected 'true' or 'false', got: '{}'", val)),
                };
            }
            "duck_level" => {
                let n = val.parse::<u32>()
                    .map_err(|_| format!("invalid integer: '{}'", val))?;
                if !(config::DUCK_LEVEL_MIN..=config::DUCK_LEVEL_MAX).contains(&n) {
                    return Err(format!("duck level must be {}-{}, got: '{}'",
                                       config::DUCK_LEVEL_MIN, config::DUCK_LEVEL_MAX, val));
                }
                self.duck_level = n;
            }
            "voice_volume" => {
                let n = val.parse::<u32>()
                    .map_err(|_| format!("invalid integer: '{}'", val))?;
                if !(config::VOICE_VOLUME_MIN..=config::VOICE_VOLUME_MAX).contains(&n) {
                    return Err(format!("voice volume must be {}-{}, got: '{}'",
                                       config::VOICE_VOLUME_MIN, config::VOICE_VOLUME_MAX, val));
                }
                self.voice_volume = n;
            }
            "llm_history" => {
                self.llm_history = match val.to_lowercase().as_str() {
                    "true"  => true,
                    "false" => false,
                    _ => return Err(format!("expected 'true' or 'false', got: '{}'", val)),
                };
            }
            "llm_history_turns" => {
                let n = val.parse::<u32>()
                    .map_err(|_| format!("invalid integer: '{}'", val))?;
                if !(config::LLM_HISTORY_TURNS_MIN..=config::LLM_HISTORY_TURNS_MAX).contains(&n) {
                    return Err(format!("history depth must be {}-{}, got: '{}'",
                                       config::LLM_HISTORY_TURNS_MIN,
                                       config::LLM_HISTORY_TURNS_MAX, val));
                }
                self.llm_history_turns = n;
            }
            "llm_history_idle_min" => {
                let n = val.parse::<u32>()
                    .map_err(|_| format!("invalid integer: '{}'", val))?;
                if !(config::LLM_HISTORY_IDLE_MIN_MIN..=config::LLM_HISTORY_IDLE_MIN_MAX).contains(&n) {
                    return Err(format!("idle minutes must be {}-{}, got: '{}'",
                                       config::LLM_HISTORY_IDLE_MIN_MIN,
                                       config::LLM_HISTORY_IDLE_MIN_MAX, val));
                }
                self.llm_history_idle_min = n;
            }
            "llm_tts_url" => {
                // The sidecar is a local process by definition - it exists
                // because the model is too big to ship - so unlike the
                // language model endpoint there is no remote case to allow.
                // A non-loopback address here would send every answer the
                // assistant speaks to somebody else's machine.
                let url = val.trim().trim_end_matches('/');
                if url.is_empty() {
                    return Err("speech sidecar url must not be empty".to_string());
                }
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    return Err(format!(
                        "speech sidecar url must start with http:// or https://: '{}'", val));
                }
                // is_loopback_url already rejects the characters that make a
                // url mean one thing here and another to reqwest, so the two
                // checks cannot disagree. Unlike llm_base_url this is enforced
                // in set() rather than validate(): there is no allow-remote
                // companion setting to make it a cross-field rule, because
                // there is no legitimate remote case.
                if !is_loopback_url(url) {
                    return Err(format!(
                        "the speech sidecar must be local: '{}' is not a loopback address", val));
                }
                self.llm_tts_url = url.to_string();
            }
            "llm_tts_mode" => {
                let mode = val.trim().to_lowercase();
                if !config::LLM_TTS_MODES.contains(&mode.as_str()) {
                    return Err(format!("expected one of {:?}, got: '{}'",
                                       config::LLM_TTS_MODES, val));
                }
                self.llm_tts_mode = mode;
            }
            "llm_tts_python" => {
                // not checked for existence here: the path is validated when
                // the sidecar is started, where a missing interpreter can be
                // reported together with the command that failed
                self.llm_tts_python = val.trim().to_string();
            }
            "llm_tts_script" => {
                self.llm_tts_script = val.trim().to_string();
            }
            "llm_tts_instruct" => {
                self.llm_tts_instruct = val.trim().to_string();
            }
            "follow_up_secs" => {
                let secs: u64 = val.trim().parse()
                    .map_err(|_| format!("expected a whole number of seconds, got: '{}'", val))?;
                if secs > config::FOLLOW_UP_SECS_MAX {
                    return Err(format!("at most {} seconds, got: {}",
                                       config::FOLLOW_UP_SECS_MAX, secs));
                }
                self.follow_up_secs = secs;
            }
            "dialogue_exit_secs" => {
                let secs: u64 = val.trim().parse()
                    .map_err(|_| format!("expected a whole number of seconds, got: '{}'", val))?;
                if !(config::DIALOGUE_EXIT_SECS_MIN..=config::DIALOGUE_EXIT_SECS_MAX)
                    .contains(&secs)
                {
                    return Err(format!("dialogue pause must be {}-{} seconds, got: {}",
                                       config::DIALOGUE_EXIT_SECS_MIN,
                                       config::DIALOGUE_EXIT_SECS_MAX, secs));
                }
                self.dialogue_exit_secs = secs;
            }
            "api_key__openai" => {
                self.api_keys.openai = val.to_string();
            }
            _ => return Err(format!("unknown setting: '{}'", key)),
        }
        Ok(())
    }

    /// clamp backend selections to ids the model registry actually knows.
    /// no-op when this process cannot judge: registry not initialized, or no
    /// models directory next to the executable (see models::check_backend).
    /// returns (key, old_value, new_value) for every field that was reset.
    pub fn sanitize_backends(&mut self) -> Vec<(&'static str, String, String)> {
        use crate::models::Task;

        let mut fixed: Vec<(&'static str, String, String)> = Vec::new();

        fix_backend("intent_backend", Task::Intent, &mut self.intent_backend, &mut fixed);
        fix_backend("slots_backend", Task::Slots, &mut self.slots_backend, &mut fixed);
        fix_backend("vad_backend", Task::Vad, &mut self.vad_backend, &mut fixed);

        fixed
    }

    /// all valid setting keys (for enumeration, debugging, etc.)
    pub fn keys() -> &'static [&'static str] {
        &[
            "selected_microphone",
            "assistant_voice",
            "selected_wake_word_engine",
            "intent_backend",
            "slots_backend",
            "vad_backend",
            "selected_gliner_model",
            "selected_vosk_model",
            "speech_to_text_engine",
            "noise_suppression",
            "gain_normalizer",
            "language",
            "ahk_interpreter",
            "llm_enabled",
            "llm_base_url",
            "llm_model",
            "llm_timeout",
            "llm_max_tokens",
            "llm_thinking",
            "llm_system_prompt",
            "llm_allow_remote",
            "llm_speak",
            "duck_others",
            "duck_level",
            "voice_volume",
            "llm_history",
            "llm_history_turns",
            "llm_history_idle_min",
            "llm_tts_url",
            "llm_tts_mode",
            "llm_tts_python",
            "llm_tts_script",
            "llm_tts_instruct",
            "follow_up_secs",
            "dialogue_exit_secs",
            "api_key__openai",
        ]
    }

    /// would this state send speech off the machine without permission?
    fn breaks_offline_first(&self) -> bool {
        !self.llm_base_url.trim().is_empty()
            && !is_loopback_url(&self.llm_base_url)
            && !self.llm_allow_remote
    }

    /// cross-field invariants for ONE save, judged as a delta against the state
    /// it replaces.
    ///
    /// this CANNOT live in set(): db_write_many hands write_many a
    /// HashMap<String,String> (jarvis-gui/src/tauri_commands/db.rs:35), whose
    /// iteration order is arbitrary. a single save that turns llm_allow_remote
    /// ON and sets a remote llm_base_url would then pass or fail depending on
    /// hash order. this runs after every pair has landed, so it is
    /// order-independent by construction.
    ///
    /// and it is a DELTA, not a check on the staged state alone, because the
    /// only thing worth refusing is a save that POINTS the assistant at a
    /// remote endpoint without permission. the other two shapes must go
    /// through:
    ///   - turning 'llm_allow_remote' back OFF while a remote url is stored.
    ///     the settings form always sends both keys, so a state check would
    ///     refuse the save that re-arms the guard and tell the user to disarm
    ///     it again. a safety switch that cannot be switched on is worse than
    ///     the state it guards.
    ///   - any unrelated single-key write (tray toggles, the language switch)
    ///     against an app.db that is already in that state. one hand-edited
    ///     value must not make every other setting in the app unwritable.
    /// neither one leaks anything: llm::LlmConfig::from_settings() refuses the
    /// endpoint again at the point of use, which is where the packet would
    /// actually leave.
    pub fn validate_change(&self, previous: &Settings) -> Result<(), String> {
        if self.breaks_offline_first()
            && !previous.breaks_offline_first()
            && self.llm_base_url != previous.llm_base_url
        {
            return Err(format!(
                "'llm_base_url': '{}' is not a loopback address. jarvis is offline-first - \
                 set 'llm_allow_remote' to true in the same save if you really mean to send \
                 your speech to another machine.",
                self.llm_base_url
            ));
        }
        Ok(())
    }
}

// ### DEFAULT

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            microphone: -1,
            voice: String::from(""),

            wake_word_engine: config::DEFAULT_WAKE_WORD_ENGINE,

            intent_backend: config::DEFAULT_INTENT_BACKEND.to_string(),
            slots_backend: config::DEFAULT_SLOTS_BACKEND.to_string(),
            vad_backend: config::DEFAULT_VAD_BACKEND.to_string(),

            gliner_model: String::new(),
            speech_to_text_engine: config::DEFAULT_SPEECH_TO_TEXT_ENGINE,
            vosk_model: String::from(""),

            noise_suppression: config::DEFAULT_NOISE_SUPPRESSION,
            gain_normalizer: config::DEFAULT_GAIN_NORMALIZER,

            language: crate::i18n::detect_system_language().to_string(),

            ahk_interpreter: String::new(),

            llm_enabled: config::DEFAULT_LLM_ENABLED,
            llm_base_url: config::DEFAULT_LLM_BASE_URL.to_string(),
            llm_model: String::new(),
            llm_timeout: config::DEFAULT_LLM_TIMEOUT,
            llm_max_tokens: config::DEFAULT_LLM_MAX_TOKENS,
            llm_thinking: config::DEFAULT_LLM_THINKING.to_string(),
            llm_system_prompt: config::DEFAULT_LLM_SYSTEM_PROMPT.to_string(),
            llm_allow_remote: config::DEFAULT_LLM_ALLOW_REMOTE,
            llm_speak: config::DEFAULT_LLM_SPEAK,
            wake_min_score: config::DEFAULT_WAKE_MIN_SCORE,
            vad_energy_threshold: config::DEFAULT_VAD_ENERGY_THRESHOLD,
            speech_pause_ms: config::DEFAULT_SPEECH_PAUSE_MS,
            duck_others: config::DEFAULT_DUCK_OTHERS,
            duck_level: config::DEFAULT_DUCK_LEVEL,
            voice_volume: config::DEFAULT_VOICE_VOLUME,
            llm_history: config::DEFAULT_LLM_HISTORY,
            llm_history_turns: config::DEFAULT_LLM_HISTORY_TURNS,
            llm_history_idle_min: config::DEFAULT_LLM_HISTORY_IDLE_MIN,
            llm_tts_url: config::DEFAULT_LLM_TTS_URL.to_string(),
            llm_tts_mode: config::DEFAULT_LLM_TTS_MODE.to_string(),
            llm_tts_python: config::DEFAULT_LLM_TTS_PYTHON.to_string(),
            llm_tts_script: String::new(),
            llm_tts_instruct: String::new(),
            follow_up_secs: config::DEFAULT_FOLLOW_UP_SECS,
            dialogue_exit_secs: config::DEFAULT_DIALOGUE_EXIT_SECS,

            api_keys: ApiKeys {
                openai: String::from(""),
            },
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ApiKeys {
    pub openai: String,
}

// ### TESTS

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_loopback_forms_the_endpoint_can_take() {
        assert!(is_loopback_url("http://127.0.0.1:1234/v1"));
        assert!(is_loopback_url("http://127.0.0.1:11434/v1"));
        assert!(is_loopback_url("http://127.5.5.5/v1"));   // the whole /8
        assert!(is_loopback_url("http://localhost:1234/v1"));
        assert!(is_loopback_url("HTTP://LocalHost:1234/v1"));
        assert!(is_loopback_url("http://[::1]:1234/v1"));
        assert!(is_loopback_url("http://user:pw@127.0.0.1:1234/v1"));
    }

    #[test]
    fn rejects_hosts_that_are_not_loopback() {
        assert!(!is_loopback_url("http://0.0.0.0:1234/v1"));
        assert!(!is_loopback_url("http://192.168.1.10:1234/v1"));
        assert!(!is_loopback_url("https://api.openai.com/v1"));
        assert!(!is_loopback_url("http://127.0.0.1.evil.com/v1"));
        assert!(!is_loopback_url("127.0.0.1:1234"));       // no scheme
        assert!(!is_loopback_url(""));
    }

    // the one that matters: a backslash makes the WHATWG parser reqwest uses
    // stop the authority at evil.com, while "text after the last @" sees
    // 127.0.0.1. if this ever passes again the gate is decoration.
    #[test]
    fn rejects_urls_that_would_parse_differently_in_reqwest() {
        assert!(!is_loopback_url(r"http://evil.com\@127.0.0.1/v1"));
        assert!(!is_loopback_url("http://evil.com\t@127.0.0.1/v1"));
        assert!(!is_loopback_url("http://evil.com\n@127.0.0.1/v1"));
        assert!(!is_loopback_url("http://evil.com\r@127.0.0.1/v1"));
        assert!(!is_loopback_url("http://evil.com @127.0.0.1/v1"));
        assert!(!is_loopback_url("http://①27.0.0.1/v1"));
    }

    #[test]
    fn set_rejects_a_url_it_cannot_reason_about() {
        let mut s = Settings::default();
        assert!(s.set("llm_base_url", r"http://evil.com\@127.0.0.1/v1").is_err());
        assert!(s.set("llm_base_url", "ws://127.0.0.1:1234/v1").is_err());
        assert!(s.set("llm_base_url", "").is_err());
        assert!(s.set("llm_base_url", "http://127.0.0.1:1234/v1/").is_ok());
        assert_eq!(s.llm_base_url, "http://127.0.0.1:1234/v1");
    }

    #[test]
    fn set_enforces_the_timeout_range() {
        let mut s = Settings::default();
        assert!(s.set("llm_timeout", "9").is_err());   // below CONNECT_TIMEOUT
        assert!(s.set("llm_timeout", "601").is_err());
        assert!(s.set("llm_timeout", "10").is_ok());
        assert_eq!(s.llm_timeout, 10);
    }

    #[test]
    fn refuses_a_save_that_points_at_a_remote_endpoint() {
        let previous = Settings::default();
        let mut staged = previous.clone();
        staged.set("llm_base_url", "https://api.openai.com/v1").unwrap();

        assert!(staged.validate_change(&previous).is_err());

        // ... unless the same save grants permission
        staged.set("llm_allow_remote", "true").unwrap();
        assert!(staged.validate_change(&previous).is_ok());
    }

    #[test]
    fn never_refuses_a_save_that_re_arms_the_guard() {
        // a remote endpoint the user already allowed
        let mut previous = Settings::default();
        previous.set("llm_allow_remote", "true").unwrap();
        previous.set("llm_base_url", "https://api.openai.com/v1").unwrap();

        // turning the guard back on, url untouched: the settings form always
        // sends both keys, so this must not be refused
        let mut staged = previous.clone();
        staged.set("llm_allow_remote", "false").unwrap();
        assert!(staged.validate_change(&previous).is_ok());

        // and an unrelated single-key write against that state must go through
        let mut unrelated = staged.clone();
        unrelated.set("gain_normalizer", "true").unwrap();
        assert!(unrelated.validate_change(&staged).is_ok());
    }
}
