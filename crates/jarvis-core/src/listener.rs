mod rustpotter;
mod vosk;

use once_cell::sync::OnceCell;
use parking_lot::Mutex;

use crate::config::structs::WakeWordEngine;

use crate::DB;

static READY: OnceCell<()> = OnceCell::new();

// Which engine handled the previous frame, so a change can be noticed.
static IN_USE: Mutex<Option<WakeWordEngine>> = Mutex::new(None);

// Both engines, always.
//
// Preparing only the chosen one is what forced a restart to change it: the
// window offered a choice, the running assistant kept whichever it had started
// with, and the two disagreed without saying so - an evening was spent
// comparing two settings that were both running Vosk. Neither engine costs
// enough to justify that. Vosk prepares nothing at all here, because it borrows
// the recogniser the command side has already loaded, and Rustpotter is six
// template files well under a megabyte between them.
pub fn init() -> Result<(), String> {
    if READY.get().is_some() {
        return Ok(());
    }

    vosk::init().map_err(|_| "Failed to init Vosk wake-word".to_string())?;
    rustpotter::init().map_err(|_| "Failed to init Rustpotter".to_string())?;

    READY.set(()).ok();
    info!("Wake word engines ready: Rustpotter and Vosk. In use: {:?}", current());
    Ok(())
}

// Read per frame rather than remembered, so the window and the microphone
// never disagree about which engine is listening.
fn current() -> WakeWordEngine {
    DB.get()
        .map(|db| db.read().wake_word_engine)
        .unwrap_or(crate::config::DEFAULT_WAKE_WORD_ENGINE)
}

pub fn data_callback(frame_buffer: &[i16]) -> Option<i32> {
    READY.get()?;
    let engine = current();

    // A switch part-way through a word leaves both engines holding a fragment,
    // and each would carry that into its next decision - the one taking over
    // would judge a name it half heard, the one stepping back would resume days
    // later mid-syllable. Clear both, and say so once rather than every frame.
    {
        let mut in_use = IN_USE.lock();
        if *in_use != Some(engine) {
            if let Some(previous) = *in_use {
                info!("Wake word engine switched from {:?} to {:?}", previous, engine);
            }
            rustpotter::reset();
            crate::stt::reset_wake_recognizer();
            *in_use = Some(engine);
        }
    }

    match engine {
        WakeWordEngine::Rustpotter => rustpotter::data_callback(frame_buffer),
        WakeWordEngine::Vosk => vosk::data_callback(frame_buffer),
    }
}
