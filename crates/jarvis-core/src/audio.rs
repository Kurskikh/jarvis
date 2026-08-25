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

// How much louder or quieter than recorded the assistant should be, in
// decibels.
//
// Decibels and not the percentage the setting is written in, because loudness
// is what the ear hears and a linear percentage is not it: halving the number
// does not halve the loudness. Kira takes decibels for the same reason.
//
// Read at every clip rather than cached, so moving the slider is heard on the
// next thing said instead of the next start.
fn voice_gain_db() -> f32 {
    match DB.get() {
        Some(db) => gain_db(db.read().voice_volume),
        // before the settings exist there is nothing to apply, and silence
        // would be the wrong guess
        None => 0.0,
    }
}

// The percentage the setting is written in, as the decibels Kira wants.
//
// Clamped here as well as when the value is stored: a hand-edited app.db is a
// supported way to break things, and a number outside the range would be a
// gain nobody chose - at the loud end, one that only distorts.
fn gain_db(percent: u32) -> f32 {
    let percent = percent.clamp(config::VOICE_VOLUME_MIN, config::VOICE_VOLUME_MAX) as f32;
    20.0 * (percent / 100.0).log10()
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
            if let Some(duration) = kira::play_sound(filename, voice_gain_db()) {
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
        AudioType::Kira => match kira::play_sequenced(wav, voice_gain_db()) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_recorded_is_no_change_at_all() {
        // 100 has to be exactly zero and not nearly zero: it is the default,
        // so every installation that never touches this setting goes through
        // it, and a stray fraction of a decibel on every clip is the kind of
        // thing nobody thinks to look for
        assert_eq!(gain_db(100), 0.0);
    }

    #[test]
    fn doubling_the_number_is_six_decibels() {
        assert!((gain_db(200) - 6.0206).abs() < 0.001);
        assert!((gain_db(50) + 6.0206).abs() < 0.001);
    }

    #[test]
    fn louder_is_a_larger_number() {
        // the direction is the whole of what a user gets wrong, and getting it
        // backwards here would be inaudible in review and obvious in use
        assert!(gain_db(150) > gain_db(100));
        assert!(gain_db(100) > gain_db(75));
    }

    #[test]
    fn a_value_from_outside_the_range_cannot_ask_for_more_than_the_ceiling() {
        assert_eq!(gain_db(10_000), gain_db(config::VOICE_VOLUME_MAX));
        assert_eq!(gain_db(0), gain_db(config::VOICE_VOLUME_MIN));
    }
}
