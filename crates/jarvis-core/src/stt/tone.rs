// T-one: streaming speech recognition for Russian, on the processor.
//
// Only for the COMMAND. The wake word stays on Vosk and is not negotiable
// here: catching one word out of a stream is done with a grammar that forces
// the decoder to choose between eight options, and a CTC model has no such
// lever. Running this one continuously to listen for a name would also mean
// decoding every sound in the room all day for the sake of one word.
//
// Measured before any of this was written, against the shipped voice clips:
// "работаю над запросом", "я не понял команду сэр", "до свидания сэр" - word
// for word, at about twenty times real time, on the processor alone. It is
// built for telephony and sherpa resamples to eight kilohertz on the way in;
// that turned out not to hurt.

use std::path::Path;

use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use sherpa_onnx::{OnlineRecognizer, OnlineRecognizerConfig, OnlineStream};

use crate::config;

static RECOGNIZER: OnceCell<OnlineRecognizer> = OnceCell::new();
static STREAM: OnceCell<Mutex<OnlineStream>> = OnceCell::new();

// This engine reports no likelihood of its own. Vosk answers with a summed
// log-likelihood and the app logs it; there is no equivalent here, and
// inventing a number that looks like one would be worse than saying so. NaN
// prints as "NaN", which is the truth, and the app only ever logs the value.
const NO_SCORE: f32 = f32::NAN;

pub fn init(model_dir: &Path) -> Result<(), String> {
    if RECOGNIZER.get().is_some() {
        return Ok(());
    }

    let model = model_dir.join("model.onnx");
    let tokens = model_dir.join("tokens.txt");
    for f in [&model, &tokens] {
        if !f.exists() {
            return Err(format!(
                "{} is missing. The weights are not in the repository - see the model's \
                 entry in settings to fetch them.",
                f.display()
            ));
        }
    }

    let mut cfg = OnlineRecognizerConfig::default();
    cfg.model_config.t_one_ctc.model = Some(model.to_string_lossy().into_owned());
    cfg.model_config.tokens = Some(tokens.to_string_lossy().into_owned());
    cfg.model_config.provider = Some("cpu".to_string());
    cfg.model_config.num_threads = config::TONE_THREADS;
    cfg.decoding_method = Some("greedy_search".to_string());

    // Endpointing, and this is the important part.
    //
    // A streaming model will not commit the end of a phrase until it has heard
    // silence after it: fed a clip with no tail, "Доброе утро, сэр" came back
    // as "доб". Rather than padding audio by hand, the recogniser is told how
    // much trailing silence ends an utterance and answers when it hears it.
    // That is also what turns a continuous stream into discrete commands.
    let pause = crate::DB
        .get()
        .map(|db| db.read().speech_pause_ms)
        .unwrap_or(config::DEFAULT_SPEECH_PAUSE_MS) as f32
        / 1000.0;
    cfg.enable_endpoint = true;
    cfg.rule1_min_trailing_silence = pause;
    cfg.rule2_min_trailing_silence = pause;
    cfg.rule3_min_utterance_length = config::TONE_MAX_UTTERANCE_SECS;

    let rec = OnlineRecognizer::create(&cfg)
        .ok_or_else(|| format!("sherpa-onnx refused the model at {}", model_dir.display()))?;
    let stream = rec.create_stream();

    RECOGNIZER.set(rec).map_err(|_| "T-one recogniser already set")?;
    STREAM.set(Mutex::new(stream)).map_err(|_| "T-one stream already set")?;
    info!("T-one ready ({} thread(s), a phrase ends after {:.2}s of silence)",
          config::TONE_THREADS, pause);
    Ok(())
}

// Feed a frame; answer only when the utterance has ended.
//
// Same contract as the Vosk path: None while the person is still talking, Some
// once there is something finished to act on.
pub fn recognize_speech(data: &[i16]) -> Option<(String, f32)> {
    let rec = RECOGNIZER.get()?;
    let stream = STREAM.get()?.lock();

    // i16 from the recorder, f32 for sherpa. The rate is the recorder's, not
    // the model's - sherpa resamples, and telling it the wrong rate here would
    // simply play the audio at the wrong speed.
    let samples: Vec<f32> = data.iter().map(|&s| s as f32 / 32768.0).collect();
    stream.accept_waveform(config::RECOGNISER_SAMPLE_RATE as i32, &samples);

    while rec.is_ready(&stream) {
        rec.decode(&stream);
    }

    if !rec.is_endpoint(&stream) {
        return None;
    }

    let text = rec
        .get_result(&stream)
        .map(|r| r.text.trim().to_string())
        .unwrap_or_default();
    // the endpoint has been consumed either way, or every later frame would
    // report it again
    rec.reset(&stream);

    if text.is_empty() {
        return None;
    }
    Some((text, NO_SCORE))
}

// Throw away whatever is half-heard.
//
// Called when a turn ends or is abandoned, so the next one does not begin with
// the tail of the last.
pub fn reset_speech_recognizer() {
    let (Some(rec), Some(stream)) = (RECOGNIZER.get(), STREAM.get()) else {
        return;
    };
    rec.reset(&stream.lock());
}
