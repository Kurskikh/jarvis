// Silero VAD, through the sherpa-onnx runtime that T-one already brought in.
//
// The energy VAD asks whether a frame is loud; this asks whether it is speech.
// The difference is the two failure modes that setting a loudness threshold
// could never fix at the same time: a fan or music opening the pipeline, and a
// quiet "джарвис" failing to. About two megabytes of weights, under a
// millisecond per frame on one processor thread.
//
// The smoothing - minimum speech length, minimum silence length - lives inside
// sherpa's detector, so what comes out of detect() is already a steady
// "somebody is speaking" rather than a per-frame flicker. The cost of that
// steadiness: is_voice stays true for MIN_SILENCE after speech actually stops,
// so every silence timer counting on top of it runs about that much longer
// than it does with the energy VAD.

use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

use crate::config;

// the descriptor id, as stored in settings and matched by vad.rs
const MODEL_ID: &str = "silero-vad";

static DETECTOR: OnceCell<Mutex<VoiceActivityDetector>> = OnceCell::new();

pub fn init() -> Result<(), String> {
    if DETECTOR.get().is_some() {
        return Ok(());
    }

    // resolved through the catalogue, not a hardcoded folder: the id is the
    // contract settings store, the directory name is only a convention, and
    // the registry is what ties the two together
    let Some(dir) = crate::models::model_dir(MODEL_ID) else {
        return Err(format!(
            "no usable '{}' model in the catalogue. The weights are not in the \
             repository - see models/app/catalog/{}/model.toml for where to \
             fetch them.",
            MODEL_ID, MODEL_ID
        ));
    };
    let model = dir.join("model.onnx");

    let cfg = VadModelConfig {
        silero_vad: SileroVadModelConfig {
            model: Some(model.to_string_lossy().into_owned()),
            threshold: config::SILERO_VAD_THRESHOLD,
            min_silence_duration: config::SILERO_VAD_MIN_SILENCE_SECS,
            min_speech_duration: config::SILERO_VAD_MIN_SPEECH_SECS,
            window_size: config::SILERO_VAD_WINDOW_SIZE,
            max_speech_duration: config::SILERO_VAD_MAX_SPEECH_SECS,
        },
        sample_rate: config::RECOGNISER_SAMPLE_RATE as i32,
        num_threads: 1,
        provider: Some("cpu".to_string()),
        ..Default::default()
    };

    let detector = VoiceActivityDetector::create(&cfg, config::SILERO_VAD_BUFFER_SECS)
        .ok_or_else(|| format!("sherpa-onnx refused the model at {}", model.display()))?;

    DETECTOR
        .set(Mutex::new(detector))
        .map_err(|_| "Silero VAD already set".to_string())?;
    info!("Silero VAD ready ({})", model.display());
    Ok(())
}

// None when the model never loaded, so the caller can fall back to the energy
// VAD rather than pretend an answer.
pub fn detect(input: &[i16]) -> Option<(bool, f32)> {
    let detector = DETECTOR.get()?.lock();

    // i16 from the recorder, f32 in [-1, 1] for the model
    let samples: Vec<f32> = input.iter().map(|&s| s as f32 / 32768.0).collect();
    detector.accept_waveform(&samples);

    // Completed segments are nobody's business here - the app does its own
    // buffering - but leaving them queued would grow that queue all day.
    detector.clear();

    let is_voice = detector.detected();
    // sherpa's C API reports the decision, not the probability behind it
    Some((is_voice, if is_voice { 1.0 } else { 0.0 }))
}

pub fn reset() {
    if let Some(d) = DETECTOR.get() {
        d.lock().reset();
    }
}
