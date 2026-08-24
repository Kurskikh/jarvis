use crate::config;
use crate::DB;

// Loudness, and nothing else.
//
// This asks whether the frame is loud, not whether it is speech, which is why
// the threshold has to be reachable from outside: a noisy room wakes the
// assistant on a fan, a quiet voice goes unheard, and the right number depends
// on the microphone and the room.
//
// Read per frame rather than cached. It is one read behind a lock the audio
// loop already takes, and caching it would mean the setting took effect only
// after a restart - which is exactly the shape of bug this project has been
// bitten by twice.
fn threshold() -> f32 {
    DB.get()
        .map(|db| db.read().vad_energy_threshold)
        .unwrap_or(config::DEFAULT_VAD_ENERGY_THRESHOLD) as f32
}

pub fn detect(input: &[i16]) -> (bool, f32) {
    let rms = calculate_rms(input);
    let threshold = threshold();
    let is_voice = rms > threshold;

    // normalize confidence to 0-1 range (rough approximation)
    let confidence = (rms / (threshold * 2.0)).min(1.0);

    (is_voice, confidence)
}

fn calculate_rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    
    let sum: f64 = samples.iter()
        .map(|&s| (s as f64).powi(2))
        .sum();
    
    (sum / samples.len() as f64).sqrt() as f32
}