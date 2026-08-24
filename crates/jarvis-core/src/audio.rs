mod kira;
mod rodio;

use once_cell::sync::OnceCell;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::config::structs::AudioType;
use crate::{config, DB, SOUND_DIR};

static AUDIO_TYPE: OnceCell<AudioType> = OnceCell::new();

// The microphone hears the speakers. While a reaction is playing, the recogniser
// transcribes the assistant's own voice and the result is handed to the command
// matcher - "запрос выполнен сэр" (ok3.wav) arriving as the user's next command,
// answered with not_found. Chaining makes it routine, because the mic is left
// open precisely when the confirmation plays.
//
// This is a playback-referenced gate, not echo cancellation: it deafens for the
// length of the clip rather than subtracting the signal, so the assistant cannot
// be interrupted while it speaks. Real AEC is a separate problem.
static SPEAKING_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

// true while a reaction is still coming out of the speakers
pub fn is_speaking() -> bool {
    match *SPEAKING_UNTIL.lock().unwrap() {
        Some(until) => Instant::now() < until,
        None => false,
    }
}

fn mark_speaking(duration: Duration) {
    // a short tail covers room decay the mic still picks up after the clip ends
    let until = Instant::now() + duration + Duration::from_millis(250);

    let mut guard = SPEAKING_UNTIL.lock().unwrap();

    // sounds can overlap; never shorten a gate that is already running
    if guard.map_or(true, |current| until > current) {
        *guard = Some(until);
    }
}

pub fn init() -> Result<(), ()> {
    if AUDIO_TYPE.get().is_some() {
        return Ok(());
    } // already initialized

    // set default audio type
    // @TODO. Make it configurable?
    AUDIO_TYPE.set(config::DEFAULT_AUDIO_TYPE).unwrap();

    // load given audio backend
    match AUDIO_TYPE.get().unwrap() {
        AudioType::Rodio => {
            // Init Rodio
            info!("Initializing Rodio audio backend.");

            match rodio::init() {
                Ok(_) => {
                    info!("Successfully initialized Rodio audio backend.");
                }
                Err(()) => {
                    error!("Failed to initialize Rodio audio backend.");

                    return Err(());
                }
            }
        }
        AudioType::Kira => {
            // Init Kira
            info!("Initializing Kira audio backend.");

            match kira::init() {
                Ok(_) => {
                    info!("Successfully initialized Kira audio backend.");
                }
                Err(_msg) => {
                    error!("Failed to initialize Kira audio backend.");

                    return Err(());
                }
            }
        }
    }

    Ok(())
}

pub fn play_sound(filename: &PathBuf) {
    let audio_type = match AUDIO_TYPE.get() {
        Some(t) => t,
        None => {
            warn!("Audio not initialized, cannot play: {}", filename.display());
            return;
        }
    };
    
    info!("Playing {}", filename.display());

    match audio_type {
        AudioType::Rodio => {
            rodio::play_sound(filename, true);
        }
        AudioType::Kira => {
            if let Some(duration) = kira::play_sound(filename) {
                mark_speaking(duration);
            }
        }
    }
}

// Speak one piece of a synthesised answer, gaplessly after whatever is
// already queued. Unlike play_sound this takes bytes: the audio arrives from
// the speech sidecar over a socket and is never a file.
//
// Returns false when the backend cannot do it, so the caller can stop asking
// for the rest of the answer instead of synthesising into silence.
pub fn play_speech(wav: Vec<u8>) -> bool {
    let Some(audio_type) = AUDIO_TYPE.get() else {
        warn!("Audio not initialized, cannot speak");
        return false;
    };

    match audio_type {
        AudioType::Kira => match kira::play_sequenced(wav) {
            Some(remaining) => {
                // the gate has to cover everything queued, not just this
                // piece, or the microphone opens between sentences and the
                // assistant hears itself finish its own answer
                mark_speaking(remaining);
                true
            }
            None => false,
        },
        AudioType::Rodio => {
            // gapless scheduling needs the clock; rodio has no equivalent here
            warn!("Speaking answers requires the Kira backend");
            false
        }
    }
}

// Cut a spoken answer short. Safe to call when nothing is speaking.
pub fn stop_speech() {
    if let Some(AudioType::Kira) = AUDIO_TYPE.get() {
        kira::stop_sequenced();
    }
    // reopen the microphone immediately: the reason to stop is to say
    // something else
    *SPEAKING_UNTIL.lock().unwrap() = None;
}

pub fn get_sound_directory() -> Option<PathBuf> {
    let db = DB.get()?;

    let voice_path = {
        let s = db.read();
        SOUND_DIR.join(&s.voice)
    };

    match voice_path.exists() {
        true => Some(voice_path),
        _ => {
            error!("No sounds folder found. Search path - {:?}", voice_path);
            None
        }
    }
}
