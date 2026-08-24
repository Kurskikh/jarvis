// A measurement, not a feature: does the Silero VAD, fed the way the app
// feeds it, actually say "speech" on real speech?
//
//   cargo run -p jarvis-core --example silero_probe --features sherpa -- <wav>...
//
// Reads each wav (mono, any rate - naively resampled to 16 kHz), pushes it in
// the app's own 512-sample frames, and reports two things per file: on how
// many frames detected() stood true, and how many finished segments the
// detector produced. The two separate a model that never judged the audio as
// speech from an integration that asks the wrong question about it.
//
// Two feeding modes, because the app calls clear() after every frame and a
// probe that skips that would not be probing the app.

use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

fn read_wav_16k(path: &str) -> Result<Vec<f32>, String> {
    let mut reader = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    if spec.channels != 1 {
        return Err(format!("{} channels; this wants mono", spec.channels));
    }
    let samples: Vec<f32> = reader
        .samples::<i16>()
        .filter_map(Result::ok)
        .map(|s| s as f32 / 32768.0)
        .collect();

    // naive linear resample; a VAD does not care about fidelity
    let rate = spec.sample_rate as f32;
    if spec.sample_rate == 16_000 {
        return Ok(samples);
    }
    let ratio = rate / 16_000.0;
    let out_len = (samples.len() as f32 / ratio) as usize;
    let out = (0..out_len)
        .map(|i| {
            let pos = i as f32 * ratio;
            let a = pos as usize;
            let b = (a + 1).min(samples.len() - 1);
            let frac = pos - a as f32;
            samples[a] * (1.0 - frac) + samples[b] * frac
        })
        .collect();
    Ok(out)
}

fn probe(detector: &VoiceActivityDetector, samples: &[f32], clear_each_frame: bool) -> (usize, usize, usize) {
    detector.reset();
    let mut frames = 0;
    let mut voiced = 0;
    let mut segments = 0;
    for chunk in samples.chunks_exact(512) {
        detector.accept_waveform(chunk);
        while !detector.is_empty() {
            segments += 1;
            detector.pop();
        }
        if clear_each_frame {
            detector.clear();
        }
        frames += 1;
        if detector.detected() {
            voiced += 1;
        }
    }
    detector.flush();
    while !detector.is_empty() {
        segments += 1;
        detector.pop();
    }
    (frames, voiced, segments)
}

fn detector_with(model: &str, threshold: f32) -> VoiceActivityDetector {
    let cfg = VadModelConfig {
        silero_vad: SileroVadModelConfig {
            model: Some(model.to_string()),
            threshold,
            min_silence_duration: 0.5,
            min_speech_duration: 0.1,
            window_size: 512,
            max_speech_duration: 20.0,
        },
        sample_rate: 16_000,
        num_threads: 1,
        provider: Some("cpu".to_string()),
        ..Default::default()
    };
    VoiceActivityDetector::create(&cfg, 30.0)
        .unwrap_or_else(|| panic!("sherpa-onnx refused the model at {}", model))
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
}

fn main() {
    let model = "models/app/catalog/silero-vad/model.onnx";
    let thresholds = [0.5f32, 0.35, 0.25, 0.15];
    let gains = [1.0f32, 0.3, 0.1, 0.03, 0.01];

    println!("loaded: {}", model);

    for path in std::env::args().skip(1) {
        match read_wav_16k(&path) {
            Err(e) => println!("{}\n  skipped: {}", path, e),
            Ok(samples) => {
                let secs = samples.len() as f32 / 16_000.0;
                println!("\n{}  ({:.1}s, full-scale RMS {:.4})", path, secs, rms(&samples));
                println!("  gain    peer-RMS  {}",
                         thresholds.iter().map(|t| format!("thr={:<5}", t))
                             .collect::<Vec<_>>().join(" "));
                for gain in gains {
                    let quiet: Vec<f32> = samples.iter().map(|s| s * gain).collect();
                    let row: Vec<String> = thresholds
                        .iter()
                        .map(|&t| {
                            let d = detector_with(model, t);
                            let (frames, voiced, segs) = probe(&d, &quiet, true);
                            format!("{:>3}/{:<3}s{}", voiced, frames, segs)
                        })
                        .collect();
                    // peer-RMS in the recorder's own i16 units, comparable to
                    // the energy VAD's threshold setting
                    println!("  x{:<5}  {:>7.0}  {}", gain, rms(&quiet) * 32768.0, row.join(" "));
                }
            }
        }
    }

    // and the other side of the trade: low thresholds must not invent speech
    let silence = vec![0.0f32; 16_000 * 3];
    let noise: Vec<f32> = (0..16_000 * 3)
        .map(|t| ((t as f32 * 0.7).sin() * (t as f32 * 0.013).cos()) * 0.05)
        .collect();
    for (name, audio) in [("silence", &silence), ("synthetic noise", &noise)] {
        let row: Vec<String> = thresholds
            .iter()
            .map(|&t| {
                let d = detector_with(model, t);
                let (frames, voiced, segs) = probe(&d, audio, true);
                format!("thr={}: {}/{} s{}", t, voiced, frames, segs)
            })
            .collect();
        println!("\n{}: {}", name, row.join("  "));
    }
}
