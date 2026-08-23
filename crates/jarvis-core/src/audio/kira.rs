use once_cell::sync::OnceCell;
use std::path::PathBuf;
use std::sync::Mutex;

// use kira::{
//     manager::{backend::DefaultBackend, AudioManager, AudioManagerSettings},
//     sound::static_sound::{StaticSoundData, StaticSoundSettings},
// };

use kira::{
    AudioManager, AudioManagerSettings, DefaultBackend,
    sound::static_sound::StaticSoundData,
};

static MANAGER: OnceCell<Mutex<AudioManager>> = OnceCell::new();

pub fn init() -> Result<(), ()> {
    if MANAGER.get().is_some() {
        return Ok(());
    }  // already initialized

    // Create an audio manager. This plays sounds and manages resources.
    match AudioManager::<DefaultBackend>::new(AudioManagerSettings::default()) {
        Ok(manager) => {
            // store
            MANAGER.set(Mutex::new(manager)).ok();

            // success
            Ok(())
        }
        Err(msg) => {
            error!("Failed to initialize audio stream.\nError details: {}", msg);

            // failed
            Err(())
        }
    }
}

// @TODO. Cache sounds in memory? With a pool of a certain size, for instance.
// returns how long the clip will play, so the caller can stop listening for
// that long - the microphone hears the speakers
pub fn play_sound(filename: &PathBuf) -> Option<std::time::Duration> {
    // load the file
    match StaticSoundData::from_file(filename) {
        Ok(sound_data) => {
            let duration = sound_data.duration();

            // play it (non-blocking)
            if let Some(manager) = MANAGER.get() {
                if let Ok(mut audio_manager) = manager.lock() {
                    if let Err(e) = audio_manager.play(sound_data) {
                        warn!("Failed to play sound: {}", e);
                        return None;
                    }
                }
            } else {
                warn!("Audio manager not initialized");
                return None;
            }

            Some(duration)
        }
        Err(err) => {
            warn!("Cannot find sound file: {} (err: {})", filename.display(), err);
            None
        }
    }
}