// Does T-one understand the speech this assistant actually hears?
//
// It is built for telephony - eight kilohertz, phone-band audio - and the
// microphone here is nothing like that. Everything else about swapping the
// command recogniser is ordinary work; this is the one question that decides
// whether the work is worth doing, so it gets answered before any of it.
//
//   cargo run -p jarvis-core --features sherpa --example tone_probe -- <wav>...

use sherpa_onnx::{OnlineRecognizer, OnlineRecognizerConfig};

fn read_wav(path: &str) -> Result<(i32, Vec<f32>), String> {
    let mut reader = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    if spec.channels != 1 {
        return Err(format!("{} channels; this wants mono", spec.channels));
    }
    // i16 is what the recorder produces and what the voice pack is stored as
    let samples: Vec<f32> = reader
        .samples::<i16>()
        .filter_map(Result::ok)
        .map(|s| s as f32 / 32768.0)
        .collect();
    Ok((spec.sample_rate as i32, samples))
}

fn main() {
    let dir = std::env::var("TONE_DIR")
        .unwrap_or_else(|_| "resources/models/t-one-ru".to_string());

    let mut cfg = OnlineRecognizerConfig::default();
    cfg.model_config.t_one_ctc.model = Some(format!("{}/model.onnx", dir));
    cfg.model_config.tokens = Some(format!("{}/tokens.txt", dir));
    cfg.model_config.num_threads = 4;
    cfg.model_config.provider = Some("cpu".to_string());
    cfg.decoding_method = Some("greedy_search".to_string());

    let started = std::time::Instant::now();
    let Some(rec) = OnlineRecognizer::create(&cfg) else {
        eprintln!("could not create the recogniser from {}", dir);
        std::process::exit(1);
    };
    println!("model loaded in {:.1}s\n", started.elapsed().as_secs_f32());

    let files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() {
        eprintln!("give it some wav files");
        std::process::exit(2);
    }

    for path in files {
        match read_wav(&path) {
            Err(e) => println!("{}\n  skipped: {}", path, e),
            Ok((rate, samples)) => {
                let secs = samples.len() as f32 / rate as f32;
                let t0 = std::time::Instant::now();

                let stream = rec.create_stream();
                stream.accept_waveform(rate, &samples);
                // A streaming model needs to hear the end before it will say
                // it. Without a tail of silence the last tokens never leave
                // the decoder: "Доброе утро, сэр" came back as "доб". Half a
                // second is enough and costs nothing.
                let tail = vec![0.0f32; (rate as usize) * 3 / 2];
                stream.accept_waveform(rate, &tail);
                stream.input_finished();
                while rec.is_ready(&stream) {
                    rec.decode(&stream);
                }
                let text = rec
                    .get_result(&stream)
                    .map(|r| r.text)
                    .unwrap_or_default();

                let took = t0.elapsed().as_secs_f32();
                println!(
                    "{}\n  {:.1}s of audio at {} Hz, decoded in {:.2}s ({:.1}x)\n  -> {}",
                    path,
                    secs,
                    rate,
                    took,
                    secs / took.max(0.001),
                    if text.trim().is_empty() { "(nothing)" } else { text.trim() }
                );
            }
        }
    }
}
