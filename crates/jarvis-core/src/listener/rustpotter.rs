use std::sync::Mutex;

use once_cell::sync::OnceCell;
use rustpotter::Rustpotter;

use crate::{config, APP_DIR};

// store rustpotter instance
static RUSTPOTTER: OnceCell<Mutex<Rustpotter>> = OnceCell::new();

// Rustpotter accepts exactly get_samples_per_frame() samples per call and
// silently returns None for anything else (detector.rs:249). The recorder is
// fixed at 512 samples because that is what PvRecorder wants, while the
// detector derives its own length from its internal rate and MFCC frame -
// 16000 * 30 / 1000 = 480. The two never lined up, so every frame was dropped
// at the door and the wake word could not fire at all. Re-chunk here rather
// than changing the recorder, which also feeds the VAD and the recogniser.
static PENDING: Mutex<Vec<i16>> = Mutex::new(Vec::new());
static FRAME_SAMPLES: OnceCell<usize> = OnceCell::new();

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

            info!("Rustpotter: {} wakeword model(s) loaded, {} samples per frame expected.",
                  loaded, rinstance.get_samples_per_frame());

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

    let wanted = *FRAME_SAMPLES.get_or_init(|| rustpotter.get_samples_per_frame());

    // the recorder's frame size is not the detector's, so buffer the remainder
    // and hand over exactly what it asks for, as many times as it fits
    let mut pending = PENDING.lock().unwrap();
    pending.extend_from_slice(frame_buffer);

    let mut hit = None;
    while pending.len() >= wanted {
        let chunk: Vec<i16> = pending.drain(..wanted).collect();

        if let Some(detection) = rustpotter.process_samples(chunk.as_slice()) {
            if hit.is_none() {
                hit = Some(detection);
            }
        }
    }
    drop(pending);

    if let Some(detection) = hit {
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
