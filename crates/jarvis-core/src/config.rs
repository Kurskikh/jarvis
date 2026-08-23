pub mod structs;
use structs::AudioType;
use structs::RecorderType;
use structs::SpeechToTextEngine;
use structs::WakeWordEngine;

use once_cell::sync::Lazy;
use std::env;
use std::fs;
use std::path::PathBuf;

use platform_dirs::AppDirs;

#[cfg(feature="jarvis_app")]
use rustpotter::{
    AudioFmt, BandPassConfig, DetectorConfig, FiltersConfig, GainNormalizationConfig,
    RustpotterConfig, ScoreMode,
};

use crate::config::structs::NoiseSuppressionBackend;
use crate::{APP_CONFIG_DIR, APP_DIRS, APP_LOG_DIR};

#[allow(dead_code)]
pub fn init_dirs() -> Result<(), String> {
    // infer app dirs
    if APP_DIRS.get().is_some() {
        return Ok(());
    }

    // cache_dir, config_dir, data_dir, state_dir
    APP_DIRS
        .set(AppDirs::new(Some(BUNDLE_IDENTIFIER), false).unwrap())
        .unwrap();

    // setup directories
    let mut config_dir = PathBuf::from(&APP_DIRS.get().unwrap().config_dir);
    let mut log_dir = PathBuf::from(&APP_DIRS.get().unwrap().config_dir);

    // create dirs, if required
    if !config_dir.exists() {
        if fs::create_dir_all(&config_dir).is_err() {
            config_dir = env::current_dir().expect("Cannot infer the config directory");
            fs::create_dir_all(&config_dir)
                .expect("Cannot create config directory, access denied?");
        }
    }

    if !log_dir.exists() {
        if fs::create_dir_all(&log_dir).is_err() {
            log_dir = env::current_dir().expect("Cannot infer the log directory");
            fs::create_dir_all(&log_dir).expect("Cannot create log directory, access denied?");
        }
    }

    // store inferred paths
    APP_CONFIG_DIR.set(config_dir).unwrap();
    APP_LOG_DIR.set(log_dir).unwrap();

    Ok(())
}

/*
   Defaults.
*/
pub const DEFAULT_AUDIO_TYPE: AudioType = AudioType::Kira;
pub const DEFAULT_RECORDER_TYPE: RecorderType = RecorderType::PvRecorder;
pub const DEFAULT_WAKE_WORD_ENGINE: WakeWordEngine = WakeWordEngine::Rustpotter;
pub const DEFAULT_SPEECH_TO_TEXT_ENGINE: SpeechToTextEngine = SpeechToTextEngine::Vosk;

// backend defaults (string IDs)
pub const DEFAULT_INTENT_BACKEND: &str = "intent-classifier";
pub const DEFAULT_SLOTS_BACKEND: &str = "none";
pub const DEFAULT_VAD_BACKEND: &str = "energy";

pub const DEFAULT_VOICE: &str = "jarvis-remaster";
pub const SOUND_PATH: &str = "resources/sound"; // extended from SOUND_DIR (resources/sound)
pub const VOICES_PATH: &str = "voices"; // extended from SOUND_PATH (resources/sound)

pub const BUNDLE_IDENTIFIER: &str = "com.priler.jarvis";
pub const DB_FILE_NAME: &str = "app.db";
pub const LOG_FILE_NAME: &str = "log.txt";
pub const APP_VERSION: Option<&str> = option_env!("CARGO_PKG_VERSION");

/*
   Tray.
*/
pub const TRAY_ICON: &str = "32x32.png";
pub const TRAY_TOOLTIP: &str = "Jarvis Voice Assistant";

// RUSPOTTER
pub const RUSPOTTER_MIN_SCORE: f32 = 0.62;

#[cfg(feature="jarvis_app")]
pub const RUSTPOTTER_DEFAULT_CONFIG: Lazy<RustpotterConfig> = Lazy::new(|| {
    RustpotterConfig {
        fmt: AudioFmt::default(),
        detector: DetectorConfig {
            avg_threshold: 0.,
            threshold: 0.5,
            // rustpotter's own default is 5. at 30 ms per frame, 15 demanded
            // 450 ms of uninterrupted above-threshold score for a word that
            // takes about that long to say - measured recall was ~54 %.
            min_scores: 5,
            score_ref: 0.22,
            band_size: 5,
            vad_mode: None,
            score_mode: ScoreMode::Max,
            eager: false,
            // comparator_band_size: 5,
            // comparator_ref: 0.22
        },
        filters: FiltersConfig {
            gain_normalizer: GainNormalizationConfig {
                // enabled: true,
                // gain_ref: None,
                // min_gain: 0.7,
                // max_gain: 1.0,
                enabled: false, // disable, now we have separate gain normalizer implementation
                gain_ref: None,
                min_gain: 0.7,
                max_gain: 1.0,
            },
            // rustpotter's own default is disabled. 80-400 Hz keeps the
            // fundamental and discards everything above it, while consonants
            // and most formants - the detail MFCC actually discriminates on -
            // sit between 300 and 3400 Hz. The community models were almost
            // certainly built with the CLI defaults, so filtering at inference
            // that was absent at training is a mismatch on top of the loss.
            band_pass: BandPassConfig {
                enabled: false,
                low_cutoff: 80.,
                high_cutoff: 400.,
            },
        },
    }
});

// COMMANDS
pub const COMMANDS_PATH: &str = "resources/commands/";
pub const RUSTPOTTER_PATH: &str = "resources/rustpotter/";

// VOSK
// pub const VOSK_MODEL_PATH: &str = const_concat!(PUBLIC_PATH, "/vosk/model_small");
pub const VOSK_MODELS_PATH: &str = "resources/vosk";
pub const VOSK_MODEL_PATH: &str = "resources/vosk/model_small";
pub const VOSK_FETCH_PHRASE: &str = "джарвис";
pub const VOSK_MIN_RATIO: f64 = 70.0;

// 0.7 lenient, expect false positives
// 0.8 balanced
// 0.9 strict
// etc
pub const VOSK_WAKE_CONFIDENCE: f32 = 0.9;

pub const VOSK_SPEECH_RECOGNIZER_MAX_ALTERNATIVES: u16 = 3;
pub const VOSK_SPEECH_RECOGNIZER_WORDS: bool = false;
pub const VOSK_SPEECH_PARTIAL_WORDS: bool = false;

// IRE (intents recognition)
pub const INTENT_CLASSIFIER_MIN_CONFIDENCE: f64 = 0.75;


// embedding classifier
pub const EMBEDDING_MIN_CONFIDENCE: f64 = 0.70;

// AUDIO PROCESSING DEFAULTS
pub const DEFAULT_NOISE_SUPPRESSION: NoiseSuppressionBackend = NoiseSuppressionBackend::None;
pub const DEFAULT_GAIN_NORMALIZER: bool = false;

// VAD settings
pub const VAD_ENERGY_THRESHOLD: f32 = 100.0;  // RMS threshold for energy-based VAD
pub const VAD_NNNOISELESS_THRESHOLD: f32 = 0.8;  // probability threshold for nnnoiseless
pub const VAD_SILENCE_FRAMES: u32 = 15;  // frames of silence before speech end (~480ms)

// gain normalizer settings
pub const GAIN_TARGET_RMS: f32 = 3000.0;  // target RMS level
pub const GAIN_MIN: f32 = 0.5;  // minimum gain multiplier
pub const GAIN_MAX: f32 = 3.0;  // maximum gain multiplier

// nnnoiseless frame size (fixed by library)
pub const NNNOISELESS_FRAME_SIZE: usize = 480;

// LUA
pub const DEFAULT_LUA_SANDBOX: &str = "standard";
pub const DEFAULT_LUA_TIMEOUT: u64 = 10000; // ms

// ### LLM (stage 1: text answer on no-command-found)
pub const DEFAULT_LLM_ENABLED: bool = false;
// loopback literal, NOT "localhost": on Windows localhost resolves to ::1
// first while ollama binds 127.0.0.1 only, which would fail against a server
// that is actually running, with an error no message could explain
pub const DEFAULT_LLM_BASE_URL: &str = "http://127.0.0.1:1234/v1";
// seconds. generous on purpose: neither LM Studio nor ollama emits any wire
// signal for "model is loading" - the request simply blocks while the weights
// go resident, 10-60s cold on a 16GB card. a tight default would make the
// FIRST turn of every session fail. a dead endpoint still degrades in ~2s via
// the connect failure, long before this.
pub const DEFAULT_LLM_TIMEOUT: u64 = 60;
// accepted range for llm_timeout. the floor is deliberately ABOVE the client's
// CONNECT_TIMEOUT (6s): the total budget wraps the connect, so a budget below
// it would report "nothing is listening" as a timeout and send the owner after
// the wrong remedy. enforced in Settings::set and clamped again in
// llm::LlmConfig::from_settings for a hand-edited app.db.
pub const LLM_TIMEOUT_MIN: u64 = 10;
pub const LLM_TIMEOUT_MAX: u64 = 600;
pub const DEFAULT_LLM_ALLOW_REMOTE: bool = false;
// A reasoning model writes its scratchpad into the same completion budget as the
// answer. 512 was not enough for one: it produced 512 reasoning tokens, hit
// finish_reason="length" and left content empty. 2048 leaves room to think AND
// answer a short question. Non-reasoning models never approach it, so nobody
// pays for the headroom.
pub const DEFAULT_LLM_MAX_TOKENS: u32 = 2048;
// "auto" sends nothing extra and lets the model do whatever it was built to do.
// "off" appends the directive below to the system prompt.
//
// LM Studio's documented parameter list has NO reasoning switch - it is
// model, top_p, top_k, messages, temperature, max_tokens, stream, stop,
// presence_penalty, frequency_penalty, logit_bias, repeat_penalty, seed - so
// there is nothing to send in the request body that would work reliably. The
// prompt convention is what the Qwen3 chat template actually reads, and a
// model that does not know it sees one short line of instruction, which is
// harmless. This is a per-model-family lever, not a standard one; say so.
pub const DEFAULT_LLM_THINKING: &str = "auto";
pub const LLM_NO_THINK_DIRECTIVE: &str = "/no_think";
pub const LLM_MAX_TOKENS_MIN: u32 = 64;
pub const LLM_MAX_TOKENS_MAX: u32 = 32768;
pub const DEFAULT_LLM_SYSTEM_PROMPT: &str =
    "You are Jarvis, a local voice assistant. Answer in the same language the user used. \
     Be brief: two or three sentences at most, no lists and no markdown - the answer is \
     read off a small screen.";

// ETC
pub const CMD_RATIO_THRESHOLD: f64 = 75f64;
pub const CMS_WAIT_DELAY: std::time::Duration = std::time::Duration::from_secs(15);

// pub const ASSISTANT_GREET_PHRASES: [&str; 3] = ["greet1", "greet2", "greet3"];
// pub const ASSISTANT_PHRASES_TBR: [&str; 17] = [
//     "джарвис",
//     "сэр",
//     "слушаю сэр",
//     "всегда к услугам",
//     "произнеси",
//     "ответь",
//     "покажи",
//     "скажи",
//     "давай",
//     "да сэр",
//     "к вашим услугам сэр",
//     "всегда к вашим услугам сэр",
//     "запрос выполнен сэр",
//     "выполнен сэр",
//     "есть",
//     "загружаю сэр",
//     "очень тонкое замечание сэр",
// ];



pub fn get_wake_phrases(lang: &str) -> &'static [&'static str] {
    match lang {
        "ru" => &["джарвис", "джервис", "гарвис", "джарви", "гарви"],
        "ua" => &["джарвіс", "джервіс"],
        "en" => &["jarvis", "jervis"],
        _ => &["jarvis"],
    }
}

pub fn get_phrases_to_remove(lang: &str) -> &'static [&'static str] {
    match lang {
        "ru" => &[
            "джарвис", "джервис", "гарвис", "джарви", "гарви",
            "сэр", "слушаю сэр", "всегда к услугам",
            "произнеси", "ответь", "покажи", "скажи", "давай",
            "да сэр", "к вашим услугам сэр", "загружаю сэр",
        ],
        "ua" => &[
            "джарвіс", "джервіс", "сер", "слухаю сер", "завжди до послуг",
            "скажи", "покажи", "відповідай", "давай",
            "так сер", "до ваших послуг сер",
        ],
        "en" => &[
            "jarvis", "jervis", "sir", "yes sir", "at your service",
            "please", "say", "show", "tell", "hey",
        ],
        _ => &["jarvis"],
    }
}

pub fn get_wake_grammar(lang: &str) -> &'static [&'static str] {
    match lang {
        "ru" => &[
            "джарвис", "[unk]", "джон", "джони", "джей",
            "джонстон", "привет", "давай",
        ],
        "ua" => &[
            "джарвіс", "[unk]", "джон", "джоні", "джей",
            "привіт", "давай",
        ],
        "en" => &[
            "jarvis", "[unk]", "john", "johnny", "jay",
            "hello", "hey", "hi",
        ],
        _ => &["jarvis", "[unk]"],
    }
}