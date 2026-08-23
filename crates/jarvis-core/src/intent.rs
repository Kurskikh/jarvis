mod intentclassifier;
mod embeddingclassifier;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{commands::{self, JCommandsList, JCommand}, config, models};
use once_cell::sync::OnceCell;

use crate::DB;

static BACKEND: OnceCell<String> = OnceCell::new();

// set for the duration of a retrain. classify() returns None while it is set.
//
// this is not optional. during the intent-classifier window between
// clear_training_data() and the last add_training_example(), predict_intent()
// sees only the upstream crate's bootstrap examples and confidently returns an
// intent like "data_merge". app.rs takes EITHER the intent branch OR the fuzzy
// branch, so get_command_by_intent() returns None and the utterance is DROPPED
// instead of falling through to commands::fetch_command(). with the flag,
// classify() returns None and the fuzzy path runs against the already-published
// NEW list - degraded matching instead of a lost command.
static RELOADING: AtomicBool = AtomicBool::new(false);

pub fn is_reloading() -> bool {
    RELOADING.load(Ordering::Acquire)
}

// clears RELOADING on every exit path, an unwind included. a flag left set
// would disable intent classification for the rest of the process lifetime -
// exactly the silent degradation the flag exists to prevent.
struct ReloadingGuard;

impl Drop for ReloadingGuard {
    fn drop(&mut self) {
        RELOADING.store(false, Ordering::Release);
    }
}

// Never fails hard. A backend that cannot start degrades to the configured
// default and, failing that, to "none" - the same way vad::init() degrades.
//
// This is what keeps a settings value out of the caller's fatal path: registry
// validation only proves that a descriptor (and its files) exist, it cannot
// prove that ORT will accept the weights, that the tokenizer parses, or that
// there is enough memory. Those only surface here, long after the value was
// written, and the process that finds out is the headless one.
pub async fn init(commands: &Vec<JCommandsList>) -> Result<(), String> {
    if BACKEND.get().is_some() {
        return Ok(());
    }

    let requested = DB.get().unwrap().read().intent_backend.clone();
    let fallback = config::DEFAULT_INTENT_BACKEND;

    // bound before the match: the future borrows `requested`, and a scrutinee
    // temporary lives until the end of the match, which would keep that borrow
    // alive while the Ok arm moves the String out
    let attempt = init_backend(&requested, commands).await;

    let effective = match attempt {
        Ok(()) => requested,
        Err(e) => {
            error!("Intent backend '{}' failed to initialize: {}", requested, e);

            if requested != fallback {
                warn!("Falling back to intent backend '{}'", fallback);
                match init_backend(fallback, commands).await {
                    Ok(()) => fallback.to_string(),
                    Err(e2) => {
                        error!(
                            "Fallback intent backend '{}' also failed: {}. Intent recognition disabled.",
                            fallback, e2
                        );
                        "none".to_string()
                    }
                }
            } else {
                error!("Intent recognition disabled.");
                "none".to_string()
            }
        }
    };

    BACKEND.set(effective).map_err(|_| "Backend already set")?;

    Ok(())
}

async fn init_backend(backend: &str, commands: &Vec<JCommandsList>) -> Result<(), String> {
    match backend {
        "none" => {
            info!("Intent recognition disabled");
        }
        "intent-classifier" => {
            info!("Initializing IntentClassifier backend.");
            intentclassifier::init(&commands).await?;
            info!("IntentClassifier backend initialized.");
        }
        // any other value is treated as a model ID for embedding classification
        model_id => {
            info!("Initializing EmbeddingClassifier with model '{}'.", model_id);
            // try_registry(), not registry(): a missed models::init() must be an
            // error we can fall back from, not a panic
            let registry = models::try_registry()
                .ok_or_else(|| "model registry is not initialized".to_string())?;
            let model = models::embedding::load(registry, model_id)?;
            embeddingclassifier::init_with_model(model, &commands)?;
            info!("EmbeddingClassifier backend initialized.");
        }
    }

    Ok(())
}

pub async fn classify(text: &str) -> Option<(String, f64)> {
    if is_reloading() {
        return None;
    }
    match BACKEND.get()?.as_str() {
        "none" => None,
        "intent-classifier" => {
            match intentclassifier::classify(text).await {
                Ok(prediction) => {
                    let confidence = prediction.confidence.value();
                    if confidence >= config::INTENT_CLASSIFIER_MIN_CONFIDENCE {
                        Some((prediction.intent.to_string(), confidence))
                    } else {
                        None
                    }
                }
                Err(e) => {
                    error!("Intent classification error: {}", e);
                    None
                }
            }
        }
        _ => {
            match embeddingclassifier::classify(text) {
                Ok((intent_id, confidence)) => {
                    if confidence >= config::EMBEDDING_MIN_CONFIDENCE {
                        Some((intent_id, confidence))
                    } else {
                        None
                    }
                }
                Err(e) => {
                    error!("Embedding classification error: {}", e);
                    None
                }
            }
        }
    }
}

// re-train the ALREADY SELECTED backend from a new command list.
//
// deliberately does NOT re-run init_backend(): re-selection would reload the
// model and could silently degrade a working backend to "none" on a transient
// failure, which is worse than not reloading. it also must not re-run init(),
// because embeddingclassifier::init_with_model() returns Ok(()) and does
// NOTHING once CLASSIFIER is set - it would report a successful reload while
// the assistant kept matching the OLD phrases.
//
// returns true when a backend was actually re-trained.
pub async fn retrain(commands: Arc<Vec<JCommandsList>>) -> Result<bool, String> {
    let backend = match BACKEND.get() {
        Some(b) => b.clone(),
        // init() never ran in this process - nothing to re-train
        None => return Ok(false),
    };

    match backend.as_str() {
        "none" => Ok(false),

        // the only backend that needs the gate: its retrain is destructive in
        // place (see RELOADING above)
        "intent-classifier" => {
            RELOADING.store(true, Ordering::Release);
            let _guard = ReloadingGuard;

            intentclassifier::retrain(&commands).await.map(|_| true)
        }

        // embeddingclassifier::retrain() builds the whole new vector set first
        // and swaps *state.intents.write() at the very end, so classify() is
        // never exposed to a half-built model. gating it here would throw away
        // intent recognition for the entire rebuild for nothing.
        _ => embeddingclassifier::retrain(commands.clone()).await.map(|_| true),
    }
}

// unified command lookup by intent ID - works for all backends
pub fn get_command_by_intent<'a>(
    commands: &'a [JCommandsList],
    intent_id: &str,
) -> Option<(&'a PathBuf, &'a JCommand)> {
    if matches!(BACKEND.get().map(|s| s.as_str()), Some("none")) {
        return None;
    }
    commands::get_command_by_id(commands, intent_id)
}
