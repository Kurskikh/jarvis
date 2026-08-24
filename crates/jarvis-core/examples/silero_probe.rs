// A measurement, not a feature: does the Silero VAD load and answer through
// sherpa-onnx? Run against the weights in the catalogue:
//
//   cargo run -p jarvis-core --example silero_probe --features sherpa -- \
//       models/app/catalog/silero-vad/model.onnx
//
// Feeds two seconds of silence and two of broadband noise, printing what the
// detector says about each. The point is that create() accepts the file and
// accept_waveform()/detected() run - a tone is not speech and the noise may
// well be judged as nothing, which is the right answer.

use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

fn main() {
    let model = std::env::args()
        .nth(1)
        .expect("usage: silero_probe <path to model.onnx>");

    let cfg = VadModelConfig {
        silero_vad: SileroVadModelConfig {
            model: Some(model.clone()),
            threshold: 0.5,
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

    let detector = VoiceActivityDetector::create(&cfg, 30.0)
        .unwrap_or_else(|| panic!("sherpa-onnx refused the model at {}", model));
    println!("loaded: {}", model);

    let mut voiced = 0;
    let mut frames = 0;
    for second in 0..4 {
        for i in 0..31 {
            let frame: Vec<f32> = (0..512)
                .map(|j| {
                    if second < 2 {
                        0.0
                    } else {
                        // deterministic broadband-ish wobble, +/-0.3
                        let t = (second * 31 + i) * 512 + j;
                        ((t as f32 * 0.7).sin() * (t as f32 * 0.013).cos()) * 0.3
                    }
                })
                .collect();
            detector.accept_waveform(&frame);
            detector.clear();
            frames += 1;
            if detector.detected() {
                voiced += 1;
            }
        }
    }

    println!(
        "{} frames processed, detected() true on {} of them (silence first, noise after)",
        frames, voiced
    );
    println!("silero probe: OK");
}
