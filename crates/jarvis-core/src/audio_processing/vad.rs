mod none;
mod energy;
#[cfg(feature = "sherpa")]
mod silero;

use once_cell::sync::OnceCell;
use parking_lot::Mutex;

use crate::DB;

static READY: OnceCell<()> = OnceCell::new();

// Which backend answered the previous frame, so a change can be noticed and
// the stateful ones reset rather than resumed mid-syllable.
static IN_USE: Mutex<Option<String>> = Mutex::new(None);

#[cfg(feature = "nnnoiseless")]
static NNNOISELESS_STATE: OnceCell<Mutex<crate::models::nnnoiseless::NnnoiselessVAD>> = OnceCell::new();

// Every backend that can run is prepared up front, and the CHOICE is read per
// frame. Preparing only the chosen one is what forces a restart to change it:
// the window says one thing while the microphone goes on using another - the
// exact bug listener.rs documents for the wake engines, and energy.rs for its
// threshold. Nothing here costs enough to justify that: nnnoiseless is a
// buffer, Silero is two megabytes of weights.
pub fn init() {
    if READY.get().is_some() {
        return;
    }

    #[cfg(feature = "nnnoiseless")]
    NNNOISELESS_STATE
        .set(Mutex::new(crate::models::nnnoiseless::NnnoiselessVAD::new()))
        .ok();

    #[cfg(feature = "sherpa")]
    if let Err(e) = silero::init() {
        // not fatal: a selection of silero then falls back to the energy VAD,
        // so the assistant stays hard of hearing rather than deaf
        warn!("Silero VAD unavailable, the energy VAD stands in for it: {}", e);
    }

    READY.set(()).ok();
    info!("VAD ready. In use: {}", current());
}

// Read per frame rather than remembered, so the settings window and the
// microphone never disagree about which detector is listening.
fn current() -> String {
    DB.get()
        .map(|db| db.read().vad_backend.clone())
        .unwrap_or_else(|| crate::config::DEFAULT_VAD_BACKEND.to_string())
}

// returns (is_voice, confidence)
pub fn detect(input: &[i16]) -> (bool, f32) {
    let backend = current();

    // A switch part-way through a sound leaves a stateful detector holding
    // half a word; clear it rather than let it resume days later mid-syllable.
    {
        let mut in_use = IN_USE.lock();
        if in_use.as_deref() != Some(backend.as_str()) {
            if let Some(previous) = in_use.take() {
                info!("VAD switched from {} to {}", previous, backend);
                reset_backend(&previous);
            }
            reset_backend(&backend);
            *in_use = Some(backend.clone());
        }
    }

    match backend.as_str() {
        "none" => none::detect(input),
        #[cfg(feature = "nnnoiseless")]
        "nnnoiseless" => {
            if let Some(state) = NNNOISELESS_STATE.get() {
                state.lock().detect(input)
            } else {
                energy::detect(input)
            }
        }
        // the descriptor id from models/app/catalog/silero-vad/model.toml -
        // the same string settings store and silero.rs resolves its files by
        #[cfg(feature = "sherpa")]
        "silero-vad" => silero::detect(input).unwrap_or_else(|| energy::detect(input)),
        _ => energy::detect(input),
    }
}

fn reset_backend(backend: &str) {
    match backend {
        #[cfg(feature = "nnnoiseless")]
        "nnnoiseless" => {
            if let Some(state) = NNNOISELESS_STATE.get() {
                state.lock().reset();
            }
        }
        #[cfg(feature = "sherpa")]
        "silero-vad" => silero::reset(),
        _ => {}
    }
}

pub fn reset() {
    // reset what actually answered the last frame - the setting may already
    // say something else
    let in_use = IN_USE.lock().clone();
    if let Some(backend) = in_use {
        reset_backend(&backend);
    }
}
