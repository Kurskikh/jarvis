use once_cell::sync::OnceCell;
use vosk::{DecodingState, Recognizer};
use std::sync::Arc;
use parking_lot::Mutex;

use crate::{vosk_models, i18n, config, models};
use crate::models::vosk::VoskModel;
use crate::DB;

// the model Arc keeps the vosk::Model alive for the recognizers
static VOSK_MODEL: OnceCell<Arc<VoskModel>> = OnceCell::new();
static WAKE_RECOGNIZER: OnceCell<Mutex<Recognizer>> = OnceCell::new();
static SPEECH_RECOGNIZER: OnceCell<Mutex<Recognizer>> = OnceCell::new();

pub fn init_vosk() -> Result<(), String> {
    if VOSK_MODEL.get().is_some() {
        return Ok(());
    }

    let model_path = get_configured_model_path()?;
    let model_id = format!("vosk:{}", model_path.display());

    // load through registry (shared if anything else needs the same model)
    let vosk = models::vosk::load(
        models::registry(),
        &model_id,
        model_path.to_str().unwrap(),
    )?;

    // language-specific wake grammar
    let lang = i18n::get_language();
    let wake_grammar = config::get_wake_grammar(&lang);
    info!("Wake grammar for '{}': {:?}", lang, wake_grammar);

    let mut wake_recognizer = Recognizer::new_with_grammar(&vosk.model, 16000.0, wake_grammar)
        .ok_or("Failed to create wake word recognizer")?;

    wake_recognizer.set_max_alternatives(1);

    let mut speech_recognizer = Recognizer::new(&vosk.model, 16000.0)
        .ok_or("Failed to create speech recognizer")?;

    speech_recognizer.set_max_alternatives(config::VOSK_SPEECH_RECOGNIZER_MAX_ALTERNATIVES);
    speech_recognizer.set_words(config::VOSK_SPEECH_RECOGNIZER_WORDS);
    speech_recognizer.set_partial_words(config::VOSK_SPEECH_PARTIAL_WORDS);

    VOSK_MODEL.set(vosk).map_err(|_| "Model already set")?;
    WAKE_RECOGNIZER.set(Mutex::new(wake_recognizer)).map_err(|_| "Wake recognizer already set")?;
    SPEECH_RECOGNIZER.set(Mutex::new(speech_recognizer)).map_err(|_| "Speech recognizer already set")?;

    Ok(())
}


pub fn recognize_wake_word(data: &[i16]) -> Option<(String, f32)> {
    let mut recognizer = WAKE_RECOGNIZER.get()?.lock();
    
    match recognizer.accept_waveform(data) {
        Ok(DecodingState::Running) => {
            None
        }
        Ok(DecodingState::Finalized) => {
            let result = recognizer.result();
            
            if let Some(alternatives) = result.multiple() {
                if let Some(best) = alternatives.alternatives.first() {
                    if !best.text.is_empty() {
                        return Some((best.text.to_string(), best.confidence));
                    }
                }
            }
            
            None
        }
        _ => None,
    }
}


// The transcript AND how sure the recogniser was of it.
//
// The score used to be dropped on the floor here, which left the command path
// with no way at all to tell a clear utterance from a guess - a half-heard
// fragment was executed with exactly the authority of a spoken command. It is
// returned and logged now; see the note in app.rs on why it is not yet a gate.
//
// Vosk's per-alternative confidence is a summed log-likelihood, so it grows
// with the length of what was said and cannot be compared against a fixed
// number without normalising. That is why this reports rather than decides.
pub fn recognize_speech(data: &[i16]) -> Option<(String, f32)> {
    let mut recognizer = SPEECH_RECOGNIZER.get()?.lock();

    match recognizer.accept_waveform(data) {
        Ok(DecodingState::Finalized) => {
            let result = recognizer.result();
            let alternatives = result.multiple()?;
            let best = alternatives.alternatives.first()?;
            if best.text.is_empty() {
                return None;
            }
            // the runners-up are the useful part when a recognition looks
            // wrong: a confident hearing leaves them far behind, a guess does
            // not, and no amount of staring at the winner alone shows that
            if alternatives.alternatives.len() > 1 {
                let others: Vec<String> = alternatives.alternatives.iter().skip(1)
                    .map(|a| format!("{:?} {:.1}", a.text, a.confidence))
                    .collect();
                debug!("Alternatives after {:?} {:.1}: {}",
                       best.text, best.confidence, others.join(", "));
            }
            Some((best.text.to_string(), best.confidence))
        }
        _ => None,
    }
}


pub fn reset_speech_recognizer() {
    if let Some(recognizer) = SPEECH_RECOGNIZER.get() {
        recognizer.lock().reset();
    }
}

pub fn reset_wake_recognizer() {
    if let Some(recognizer) = WAKE_RECOGNIZER.get() {
        recognizer.lock().reset();
    }
}

fn get_configured_model_path() -> Result<std::path::PathBuf, String> {
    // try to get from settings
    if let Some(db) = DB.get() {
        let settings = db.read();
        if !settings.vosk_model.is_empty() {
            if let Some(path) = vosk_models::get_model_path(&settings.vosk_model) {
                return Ok(path);
            }
            warn!("Configured Vosk model '{}' not found, falling back to auto-detect", settings.vosk_model);
        }
    }
    
    // auto-detect: prefer model matching current language
    let available = vosk_models::scan_vosk_models();
    let language = i18n::get_language();

    let lang_code = match language.as_str() {
        "ru" => "ru",
        "en" => "us",
        "ua" => "uk",
        other => other,
    };

    if let Some(matched) = available.iter().find(|m| m.language == lang_code) {
        info!("Auto-detected Vosk model for '{}': {}", language, matched.name);
        return Ok(matched.path.clone());
    }

    if let Some(first) = available.first() {
        info!("Auto-detected Vosk model (no language match): {}", first.name);
        return Ok(first.path.clone());
    }
    
    // fallback to legacy path
    let legacy_path = std::path::Path::new(config::VOSK_MODEL_PATH);
    if legacy_path.exists() {
        return Ok(legacy_path.to_path_buf());
    }
    
    Err("No Vosk models found".into())
}
