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

#[cfg(feature = "vosk")]
mod vosk;
#[cfg(feature = "sherpa")]
mod tone;

use once_cell::sync::OnceCell;

use crate::config;
use crate::config::structs::SpeechToTextEngine;
use crate::DB;

// the wake path, always Vosk - re-exported unchanged
pub use self::vosk::init_vosk;
pub use self::vosk::recognize_wake_word;
pub use self::vosk::reset_wake_recognizer;

static STT_TYPE: OnceCell<SpeechToTextEngine> = OnceCell::new();

// Which engine the command path uses, decided once at startup.
//
// Once, not per call: swapping recognisers mid-turn would cut a phrase in half
// between two models. Changing it takes effect on the next start, and the
// settings screen says so.
fn engine() -> SpeechToTextEngine {
    *STT_TYPE.get().unwrap_or(&SpeechToTextEngine::Vosk)
}

pub fn init() -> Result<(), String> {
    if STT_TYPE.get().is_some() {
        return Ok(());
    }

    let wanted = DB
        .get()
        .map(|db| db.read().speech_to_text_engine)
        .unwrap_or(config::DEFAULT_SPEECH_TO_TEXT_ENGINE);

    // Vosk first and unconditionally: it hears the wake word whatever else is
    // chosen, so there is no configuration in which it can be skipped.
    info!("Initializing Vosk (wake word).");
    vosk::init_vosk()?;

    let chosen = match wanted {
        SpeechToTextEngine::Vosk => {
            info!("Commands: Vosk.");
            SpeechToTextEngine::Vosk
        }

        #[cfg(feature = "sherpa")]
        SpeechToTextEngine::TOne => {
            let dir = crate::APP_DIR.join(crate::models::MODELS_PATH).join(config::TONE_MODEL_DIR);
            match tone::init(&dir) {
                Ok(()) => {
                    info!("Commands: T-one.");
                    SpeechToTextEngine::TOne
                }
                // Not fatal. An assistant that refuses to start because one of
                // two recognisers is missing its weights is worse than one
                // that says so and carries on with the other.
                Err(e) => {
                    warn!("T-one is unavailable, commands stay on Vosk: {}", e);
                    SpeechToTextEngine::Vosk
                }
            }
        }

        #[cfg(not(feature = "sherpa"))]
        SpeechToTextEngine::TOne => {
            warn!("This build has no T-one; commands stay on Vosk.");
            SpeechToTextEngine::Vosk
        }
    };

    STT_TYPE.set(chosen).map_err(|_| "STT type already set".to_string())?;
    info!("STT backend initialized.");
    Ok(())
}

// A finished command transcript together with the recogniser's own score.
//
// Separate from recognize() because most callers only pump audio in and
// discard the result; only the one place that decides to ACT on what was heard
// needs to know how sure it was. Not every engine reports one - see the note
// on NO_SCORE in the T-one module.
pub fn recognize_command(data: &[i16]) -> Option<(String, f32)> {
    match engine() {
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
    match engine() {
        SpeechToTextEngine::Vosk => vosk::reset_speech_recognizer(),
        #[cfg(feature = "sherpa")]
        SpeechToTextEngine::TOne => tone::reset_speech_recognizer(),
        #[cfg(not(feature = "sherpa"))]
        SpeechToTextEngine::TOne => vosk::reset_speech_recognizer(),
    }
}
