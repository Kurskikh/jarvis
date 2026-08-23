mod rustpotter;
mod vosk;

use once_cell::sync::OnceCell;

use crate::config::structs::WakeWordEngine;

use crate::DB;

static WAKE_WORD_ENGINE: OnceCell<WakeWordEngine> = OnceCell::new();

pub fn init() -> Result<(), String> {
    if WAKE_WORD_ENGINE.get().is_some() {
        return Ok(());
    }

    let requested = DB.get().unwrap().read().wake_word_engine;

    // there is no Porcupine implementation, and the settings page no longer
    // offers it - but an app.db written before that still can hold it. resolve
    // to the engine that will actually run BEFORE storing it, so data_callback
    // routes to the same one. erroring here used to leave the assistant running
    // with no wake word at all.
    let engine = match requested {
        WakeWordEngine::Porcupine => {
            warn!("Porcupine wake-word engine is not supported, falling back to Rustpotter.");
            WakeWordEngine::Rustpotter
        }
        other => other,
    };

    WAKE_WORD_ENGINE.set(engine)
        .map_err(|_| "Wake word engine already set".to_string())?;

    match engine {
        WakeWordEngine::Porcupine => unreachable!("resolved away above"),
        WakeWordEngine::Rustpotter => {
            info!("Initializing Rustpotter wake-word engine.");
            rustpotter::init()
                .map_err(|_| "Failed to init Rustpotter".to_string())
        }
        WakeWordEngine::Vosk => {
            info!("Initializing Vosk as wake-word engine.");
            warn!("Using Vosk as wake-word engine is highly not recommended, because it's very slow for this task.");
            vosk::init()
                .map_err(|_| "Failed to init Vosk wake-word".to_string())
        }
    }
}

pub fn data_callback(frame_buffer: &[i16]) -> Option<i32> {
    match WAKE_WORD_ENGINE.get()? {
        WakeWordEngine::Porcupine => None,
        WakeWordEngine::Rustpotter => rustpotter::data_callback(frame_buffer),
        WakeWordEngine::Vosk => vosk::data_callback(frame_buffer),
    }
}
