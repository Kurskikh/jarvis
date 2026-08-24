// Two jobs that used to be one engine.
//
// The WAKE WORD is always Vosk, and that is not a preference. Catching one
// name out of a room's worth of sound is done by handing the decoder a grammar
// of eight words and letting it choose between them - Vosk has that lever, a
// CTC model does not. Running a full recogniser continuously to spot one word
// would mean decoding every sound in the house all day.
//
// The COMMAND is whatever the setting says. Once the assistant is listening
// deliberately, the constraint is gone and accuracy is what matters.
//
// Until now neither was true: the engine came from a constant, the command
// path called Vosk directly, and the setting the window displayed had a getter
// and no setter. Three layers agreeing to show a choice nobody could make.
//
// And once the choice worked it still needed a restart, which is its own kind
// of lie - the window said T-one while the microphone went on using Vosk. Both
// engines are now loaded at startup and the setting is read as each turn
// begins, so what is chosen is what listens.

#[cfg(feature = "vosk")]
mod vosk;
#[cfg(feature = "sherpa")]
mod tone;

use once_cell::sync::OnceCell;
use parking_lot::Mutex;

use crate::config;
use crate::config::structs::SpeechToTextEngine;
use crate::DB;

// the wake path, always Vosk - re-exported unchanged
pub use self::vosk::init_vosk;
pub use self::vosk::recognize_wake_word;
pub use self::vosk::reset_wake_recognizer;

static READY: OnceCell<()> = OnceCell::new();

// Whether T-one's weights loaded. Vosk needs no such flag; it is always here,
// because the wake word depends on it.
static TONE_READY: OnceCell<bool> = OnceCell::new();

// Which engine handled the previous turn, so a change can be noticed.
static IN_USE: Mutex<Option<SpeechToTextEngine>> = Mutex::new(None);

fn tone_ready() -> bool {
    *TONE_READY.get().unwrap_or(&false)
}

// Which engine the command path uses, read fresh rather than remembered.
//
// Falls back to Vosk when T-one has no weights on disk, so a download that
// never finished leaves the assistant hard of hearing rather than deaf.
fn wanted() -> SpeechToTextEngine {
    let setting = DB
        .get()
        .map(|db| db.read().speech_to_text_engine)
        .unwrap_or(config::DEFAULT_SPEECH_TO_TEXT_ENGINE);

    match setting {
        SpeechToTextEngine::TOne if !tone_ready() => SpeechToTextEngine::Vosk,
        chosen => chosen,
    }
}

// The engine to use now, having settled any change since the last call.
//
// A switch part-way through a phrase would otherwise cut it between two models:
// the one taking over would begin at whatever syllable the setting was saved
// on, and the one stepping back would still be holding the first half when it
// is next asked. Clearing both costs the phrase being spoken at that instant -
// the one during which the user was looking at the settings window rather than
// the microphone - and nothing after it.
fn active() -> SpeechToTextEngine {
    let engine = wanted();

    let mut in_use = IN_USE.lock();
    if *in_use != Some(engine) {
        if let Some(previous) = *in_use {
            info!("Command engine switched from {:?} to {:?}", previous, engine);
            reset_one(previous);
        }
        reset_one(engine);
        *in_use = Some(engine);
    }

    engine
}

fn reset_one(engine: SpeechToTextEngine) {
    match engine {
        SpeechToTextEngine::Vosk => vosk::reset_speech_recognizer(),
        #[cfg(feature = "sherpa")]
        SpeechToTextEngine::TOne => tone::reset_speech_recognizer(),
        #[cfg(not(feature = "sherpa"))]
        SpeechToTextEngine::TOne => vosk::reset_speech_recognizer(),
    }
}

// Load every engine there are weights for, not just the chosen one.
//
// The setting can change while the assistant is running, and loading a
// recogniser on demand would mean doing it on the audio thread - a second and a
// half of silence in the middle of listening, which is the one moment that
// cannot afford it. T-one costs about that much at startup and some memory
// afterwards; paid once, where it is not noticed.
pub fn init() -> Result<(), String> {
    if READY.get().is_some() {
        return Ok(());
    }

    // Vosk first and unconditionally: it hears the wake word whatever else is
    // chosen, so there is no configuration in which it can be skipped.
    info!("Initializing Vosk (wake word and commands).");
    vosk::init_vosk()?;

    #[cfg(feature = "sherpa")]
    {
        let dir = crate::APP_DIR.join(crate::models::MODELS_PATH).join(config::TONE_MODEL_DIR);
        match tone::init(&dir) {
            Ok(()) => {
                info!("T-one ready for commands.");
                TONE_READY.set(true).ok();
            }
            // Not fatal. An assistant that refuses to start because one of two
            // recognisers is missing its weights is worse than one that says so
            // and carries on with the other.
            Err(e) => {
                warn!("T-one is unavailable, commands stay on Vosk: {}", e);
                TONE_READY.set(false).ok();
            }
        }
    }

    #[cfg(not(feature = "sherpa"))]
    {
        warn!("This build has no T-one; commands stay on Vosk.");
        TONE_READY.set(false).ok();
    }

    READY.set(()).ok();
    info!("STT backends initialized. Commands: {:?}", wanted());
    Ok(())
}

// A finished command transcript together with the recogniser's own score.
//
// Separate from recognize() because most callers only pump audio in and
// discard the result; only the one place that decides to ACT on what was heard
// needs to know how sure it was. Not every engine reports one - see the note
// on NO_SCORE in the T-one module.
pub fn recognize_command(data: &[i16]) -> Option<(String, f32)> {
    match active() {
        SpeechToTextEngine::Vosk => vosk::recognize_speech(data),
        #[cfg(feature = "sherpa")]
        SpeechToTextEngine::TOne => tone::recognize_speech(data),
        #[cfg(not(feature = "sherpa"))]
        SpeechToTextEngine::TOne => vosk::recognize_speech(data),
    }
}

pub fn recognize(data: &[i16], include_partial: bool) -> Option<String> {
    if include_partial {
        // the wake path, always Vosk
        vosk::recognize_wake_word(data).map(|(text, _)| text)
    } else {
        recognize_command(data).map(|(text, _)| text)
    }
}

// Throw away a half-heard command, so the next turn does not begin with the
// tail of the last.
pub fn reset_speech_recognizer() {
    reset_one(active());
}
