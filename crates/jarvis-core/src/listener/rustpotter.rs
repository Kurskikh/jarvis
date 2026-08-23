use std::sync::Mutex;

use once_cell::sync::OnceCell;
use rustpotter::Rustpotter;

use crate::{config, APP_DIR};

// store rustpotter instance
static RUSTPOTTER: OnceCell<Mutex<Rustpotter>> = OnceCell::new();

pub fn init() -> Result<(), ()> {
    let rustpotter_config = config::RUSTPOTTER_DEFAULT_CONFIG;

    // create rustpotter instance
    match Rustpotter::new(&rustpotter_config) {
        Ok(mut rinstance) => {
            // success
            // every wakeword model we ship for "Джарвис". ScoreMode::Max means the
            // best-scoring one wins, so loading all of them raises recall without
            // making a false positive any more likely than the loosest model alone.
            // @TODO. Make it configurable via GUI for custom user voice.
            let rustpotter_wake_word_files: [&str; 6] = [
                "jarvis-default.rpw",
                "jarvis-community-1.rpw",
                "jarvis-community-2.rpw",
                "jarvis-community-3.rpw",
                "jarvis-community-4.rpw",
                "jarvis-community-5.rpw",
            ];

            // load wake word files, resolved against APP_DIR like every other
            // resource - a relative path here would depend on the working
            // directory the app happened to be launched from
            let mut loaded = 0;
            for rpw in rustpotter_wake_word_files {
                let path = APP_DIR.join(config::RUSTPOTTER_PATH).join(rpw);

                // the file name doubles as the detection key, so the log says
                // which model fired
                match rinstance.add_wakeword_from_file(rpw, path.to_string_lossy().as_ref()) {
                    Ok(_) => loaded += 1,
                    Err(e) => error!("Failed to load wakeword file '{}': {}", path.display(), e),
                }
            }

            if loaded == 0 {
                error!("No Rustpotter wakeword models loaded, wake word detection will never fire.");
                return Err(());
            }

            info!("Rustpotter: {} wakeword model(s) loaded.", loaded);

            // store
            let _ = RUSTPOTTER.set(Mutex::new(rinstance));
        }
        Err(msg) => {
            error!("Rustpotter failed to initialize.\nError details: {}", msg);

            return Err(());
        }
    }

    Ok(())
}

pub fn data_callback(frame_buffer: &[i16]) -> Option<i32> {
    let mut lock = RUSTPOTTER.get().unwrap().lock();
    let rustpotter = lock.as_mut().unwrap();
    // let detection = rustpotter.process_samples(frame_buffer.to_vec()); // @TODO. Temp crutch. Fix optimization issue, frame_buffer should not be copied to a new vector!
    let detection = rustpotter.process_samples(frame_buffer);

    // info!("Ruspotter data callback");

    if let Some(detection) = detection {
        // one readable line per candidate: which model scored, how strongly,
        // and the gate it had to clear. near-misses stay at debug, so tuning
        // the threshold is a matter of reading the log rather than guessing
        if detection.score > config::RUSPOTTER_MIN_SCORE {
            info!("Wake word: '{}' score {:.3} (min {:.2}) - DETECTED",
                  detection.name, detection.score, config::RUSPOTTER_MIN_SCORE);

            return Some(0);
        } else {
            debug!("Wake word: '{}' score {:.3} (min {:.2}) - below threshold",
                   detection.name, detection.score, config::RUSPOTTER_MIN_SCORE);
        }
    }
    None
}
