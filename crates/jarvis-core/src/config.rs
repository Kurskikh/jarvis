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

// How closely the wake word has to match before the assistant answers.
//
// This sits on top of rustpotter's own threshold and is the gate that decides.
// A setting rather than a constant because one number cannot be right for
// every voice, microphone and room - measured here, a genuine "джарвис" scored
// 0.616 against a gate of 0.62 and was missed by four thousandths, so it had
// to be said twice and the start of the command was lost with the first try.
//
// Stored in hundredths because that is what a NumberInput can offer without
// arguing about decimal separators; divided on the way out.
pub const DEFAULT_WAKE_MIN_SCORE: u32 = 62;
pub const WAKE_MIN_SCORE_MIN: u32 = 30;
pub const WAKE_MIN_SCORE_MAX: u32 = 95;

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
pub const RUSTPOTTER_PATH: &str = "models/app/rustpotter/";

// VOSK
// pub const VOSK_MODEL_PATH: &str = const_concat!(PUBLIC_PATH, "/vosk/model_small");
pub const VOSK_MODELS_PATH: &str = "models/app/vosk";
pub const VOSK_MODEL_PATH: &str = "models/app/vosk/model_small";
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
pub const VAD_NNNOISELESS_THRESHOLD: f32 = 0.8;  // probability threshold for nnnoiseless

// Silero VAD, through sherpa-onnx. It answers "is this speech", where the
// energy VAD answers "is this loud" - which is why it needs no per-room
// threshold setting the way the energy one does.
// the model's own speech probability gate; 0.5 is the author's default
pub const SILERO_VAD_THRESHOLD: f32 = 0.5;
// a pause this long ends a stretch of speech. Short pauses inside a sentence
// stay inside it, which is precisely what the frame-by-frame energy VAD could
// not do. The price: detected() holds "voice" for this long after speech
// actually stops, so every silence timer built on top of is_voice runs this
// much longer than it does with the energy VAD.
pub const SILERO_VAD_MIN_SILENCE_SECS: f32 = 0.5;
// speech shorter than this is a click or a cough, not a voice
pub const SILERO_VAD_MIN_SPEECH_SECS: f32 = 0.1;
// the model consumes exactly this many samples per step at 16 kHz - the same
// 512 the recorder already produces per frame
pub const SILERO_VAD_WINDOW_SIZE: i32 = 512;
// tied to the recogniser's cap on purpose: the VAD must never split speech the
// recogniser is still willing to hear as one utterance
pub const SILERO_VAD_MAX_SPEECH_SECS: f32 = TONE_MAX_UTTERANCE_SECS;
// sherpa's internal sample buffer. It has to hold the longest IN-PROGRESS
// stretch of speech - samples only leave it when a segment closes, so it must
// comfortably exceed SILERO_VAD_MAX_SPEECH_SECS or continuous speech overflows
// it. About two megabytes at this size; cheap next to running out.
pub const SILERO_VAD_BUFFER_SECS: f32 = 30.0;

// How loud a frame has to be before it counts as speech, as RMS across the
// frame. A setting rather than a constant because it cannot be right for
// everyone: it is a bare loudness comparison with no notion of speech at all,
// so a noisy room wakes the assistant on a fan and a quiet voice goes unheard.
// The right value depends on the microphone and the room, which is exactly
// what a setting is for.
pub const DEFAULT_VAD_ENERGY_THRESHOLD: u32 = 100;
pub const VAD_ENERGY_THRESHOLD_MIN: u32 = 10;
pub const VAD_ENERGY_THRESHOLD_MAX: u32 = 2000;

// How long a pause ends a phrase.
//
// Streaming recognition will not commit the end of an utterance until it has
// heard silence after it - measured on T-one, a clip fed with no trailing
// silence returned "доб" where the speech said "Доброе утро, сэр". This is
// what it waits for. Vosk decides this for itself and ignores the setting.
pub const DEFAULT_SPEECH_PAUSE_MS: u32 = 800;
pub const SPEECH_PAUSE_MS_MIN: u32 = 200;
pub const SPEECH_PAUSE_MS_MAX: u32 = 3000;

// What the recorder produces and what every recogniser here is told to expect.
// sherpa resamples to whatever its model wants; saying the wrong number here
// would simply play the audio at the wrong speed.
pub const RECOGNISER_SAMPLE_RATE: u32 = 16_000;

// T-one runs on the processor. Four threads keeps it at about twenty times
// real time without taking the machine over.
// where the weights live, under the model catalogue
pub const TONE_MODEL_DIR: &str = "t-one-ru";
pub const TONE_THREADS: i32 = 4;
// A spoken command that has run this long without a pause is not a command.
pub const TONE_MAX_UTTERANCE_SECS: f32 = 20.0;

// How far behind a wake-word hit the recogniser's audio has to start. It must
// cover the whole name plus the detector's own lag - the detector reaches its
// verdict near the END of the name - or the first transcript holds a fragment
// the wake-phrase list cannot match.
pub const WAKE_NAME_COVER_SECS: f32 = 1.5;

// The most audio the recogniser is ever primed with at activation. The primer
// is normally bounded by the current voice episode - speech since the VAD last
// opened - so this cap only bites when someone talks straight through into the
// name. Long enough for a whole sentence that ends in the name; short enough
// that a monologue does not arrive labelled as a command.
pub const WAKE_ASR_PRIMER_MAX_SECS: f32 = 4.0;

// How long the priming decode may hold the audio thread. The microphone
// driver buffers about 1.6 seconds; a decode that runs longer starts losing
// the very command being spoken. T-one never comes near this; Vosk decodes at
// its own pace and must be stopped rather than trusted.
pub const WAKE_PRIMER_BUDGET_MS: u64 = 800;

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

// ### Speech (stage 2: the answer is spoken, not just written)
//
// A synthesised answer comes from a separate process. It has to: CosyVoice
// needs torch and a couple of gigabytes of model, which cannot go in an
// installer, so the sidecar is something the owner installs once and Jarvis
// talks to over loopback.
pub const DEFAULT_LLM_SPEAK: bool = true;
pub const DEFAULT_LLM_TTS_URL: &str = "http://127.0.0.1:8771";
// "stream" lets CosyVoice emit frames as it generates them; "sentence" waits
// for the whole answer and returns it as one clip.
//
// Measured on a 5070 Ti: stream reaches first speech in 1.8-2.4s and varies by
// half a second between runs, sentence takes 3.2-7.4s and varies by two and a
// half. Streaming is both faster and, more importantly, predictable. The slow
// mode is kept because it is the honest comparison, not because it is a
// fallback - the fallback is automatic and lives in the sidecar.
pub const DEFAULT_LLM_TTS_MODE: &str = "stream";
pub const LLM_TTS_MODES: [&str; 2] = ["stream", "sentence"];
// Empty means "connect to a sidecar that is already running, never start
// one". Set it to the interpreter of the environment CosyVoice is installed
// in and Jarvis will spawn and supervise the process itself.
pub const DEFAULT_LLM_TTS_PYTHON: &str = "";
// An instruction handed to the synthesiser describing HOW to speak - emotion,
// pace, manner - rather than what to say. Empty means plain voice cloning.
//
// Measured on Fun-CosyVoice3-0.5B with a Russian reference, one line, three
// languages of instruction:
//   russian  - the model READ THE INSTRUCTION ALOUD, then garbled the line
//   english  - did not read it out, but mangled the words
//   chinese  - clean
// The model is Chinese and its instruct training is Chinese, so an
// instruction in anything else is treated as text to speak. Left empty by
// default for that reason, and because instruct mode drops
// llm_prompt_speech_token: the timbre survives, the MANNER copied from the
// reference does not.
pub const DEFAULT_LLM_TTS_INSTRUCT: &str = "";
// how long to wait for a frame before deciding the sidecar has died. Generous:
// the first frame of an answer costs about two seconds and a cold sidecar
// spends ten more loading the model.
pub const LLM_TTS_FIRST_FRAME_TIMEOUT: u64 = 45;
// between frames the model is already generating, so a long gap means it
// stopped rather than that it is thinking
pub const LLM_TTS_FRAME_TIMEOUT: u64 = 30;

// Appended to the system prompt whenever the answer is going to be spoken.
//
// Kept out of DEFAULT_LLM_SYSTEM_PROMPT deliberately: that one is the owner's
// to edit, and this is a property of the output device, not of the assistant's
// character. Someone who rewrites their prompt entirely should not silently
// lose it.
//
// Punctuation is the point. CosyVoice reads rhythm and melody off commas and
// full stops, and gemma answered a real question with "У меня всё хорошо
// спасибо что спросили Я готов помочь" - not one mark in the whole line, which
// synthesises as a flat unbroken stream. Everything else here is a small
// bonus; the first sentence is the whole reason this exists.
//
// It used to permit two tags, <strong> and [breath], because CosyVoice 3
// understands them. The speech engine is Qwen3-TTS now and it does not: asked
// to say "[breath]" it says "Бред", and "<strong>" becomes "строк". The tags
// were being stripped from the window and the log while going to the
// synthesiser intact, so they were audible and invisible at the same time -
// the worst combination for working out what is wrong.
//
// So: no brackets at all. speech::say strips them anyway, because a model
// asked for plain prose will still produce one occasionally.
pub const LLM_SPEECH_STYLE_DIRECTIVE: &str = concat!(
    "This answer will be read aloud, so punctuate it properly: commas, full stops ",
    "and question marks are what give speech its rhythm, and a line without them ",
    "is read flat. Write plain prose only - no markdown, lists, emoji, parentheses, ",
    "square brackets or angle brackets of any kind. Write numbers out the way you ",
    "would say them.",
);

// ### Follow-up: keep listening after an answer
//
// Saying the wake word before every sentence of a conversation is the thing
// that makes an assistant feel like a command line. After a turn finishes the
// microphone stays open for a while, so the next question can just be asked.
//
// The window opens when the assistant STOPS TALKING, not when the command
// returns: a language model turn answers seconds later and then reads the
// answer out, and a timer started at the command would be long gone by then.
// Turning other applications down while the assistant is spoken to.
//
// On by default: it is the behaviour that makes talking over music work at
// all, and it puts every volume back afterwards. The level is what is LEFT,
// not what is taken - 20 means a fifth of its former loudness, which is close
// to what Windows itself does while a call is in progress.
pub const DEFAULT_DUCK_OTHERS: bool = true;
pub const DEFAULT_DUCK_LEVEL: u32 = 20;
pub const DUCK_LEVEL_MIN: u32 = 0;
// not 100: that would leave everything exactly as it was, which is what the
// switch above is for
pub const DUCK_LEVEL_MAX: u32 = 90;

// Dialogue memory.
//
// Off by default, and deliberately so. Remembering is not free: every exchange
// travels back to the model inside the next prompt, which costs latency on a
// local machine, and a thread that starts from a misheard question keeps
// answering the wrong one until it is cleared.
pub const DEFAULT_LLM_HISTORY: bool = false;

// How many question-and-answer pairs go with the next question.
pub const DEFAULT_LLM_HISTORY_TURNS: u32 = 4;
pub const LLM_HISTORY_TURNS_MIN: u32 = 1;
pub const LLM_HISTORY_TURNS_MAX: u32 = 20;

// A conversation ends by itself after this long without a word.
//
// Voice has no "new chat" button. Without a timeout the assistant would read
// tomorrow morning's question in the light of tonight's, and the person asking
// would have no way of knowing that is what happened.
pub const DEFAULT_LLM_HISTORY_IDLE_MIN: u32 = 5;
pub const LLM_HISTORY_IDLE_MIN_MIN: u32 = 1;
pub const LLM_HISTORY_IDLE_MIN_MAX: u32 = 240;

pub const DEFAULT_FOLLOW_UP_SECS: u64 = 8;
pub const FOLLOW_UP_SECS_MAX: u64 = 120;   // 0 disables it

// How long the dialogue waits before deciding the conversation is over.
//
// Shorter than the follow-up window on purpose. A follow-up is an afterthought
// to a command and can afford to wait; a dialogue is a conversation, and one
// that keeps the microphone open long after the last word has been said is
// listening to the room rather than to a person. The clock does not start until
// the assistant has finished speaking - see the llm_busy check in the loop.
pub const DEFAULT_DIALOGUE_EXIT_SECS: u64 = 4;
pub const DIALOGUE_EXIT_SECS_MIN: u64 = 2;
pub const DIALOGUE_EXIT_SECS_MAX: u64 = 60;

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

// Ways of saying "we are done here", checked inside the dialogue.
//
// Separate from the stop command pack, and deliberately so: inside a dialogue
// nothing is matched against commands at all, so the pack is not consulted and
// these have to stand on their own. They are compared as whole phrases against
// the whole utterance, not searched for inside it - otherwise "хватит об этом,
// расскажи про другое" would end a conversation it was trying to steer.
pub fn get_dialogue_exit_phrases(lang: &str) -> &'static [&'static str] {
    match lang {
        "ru" => &[
            "стоп", "хватит", "всё", "все", "конец", "закончили", "выход",
            "хватит болтать", "закончим", "давай закончим", "пока",
            "спасибо всё", "спасибо все", "до свидания",
        ],
        "ua" => &[
            "стоп", "досить", "все", "кінець", "закінчили", "вихід",
            "закінчимо", "давай закінчимо", "бувай", "до побачення",
        ],
        "en" => &[
            "stop", "enough", "that is all", "thats all", "we are done",
            "were done", "goodbye", "bye", "exit", "end", "thank you that is all",
        ],
        _ => &["stop"],
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