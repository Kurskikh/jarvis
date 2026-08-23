use intent_classifier::{
    IntentPrediction, IntentError,
    TrainingExample, TrainingSource, IntentId
};

use std::sync::Arc;
use std::fs;

use crate::commands::{self, JCommandsList};
use crate::models;
use crate::models::intent_classifier::IntentClassifierModel;
use crate::{APP_CONFIG_DIR, i18n};

use once_cell::sync::OnceCell;

static MODEL: OnceCell<Arc<IntentClassifierModel>> = OnceCell::new();

const TRAINING_CACHE_FILE: &str = "intent_training.json";
const COMMANDS_HASH_FILE: &str = "commands_hash.txt";

pub async fn init(commands: &[JCommandsList]) -> Result<(), String> {
    let current_hash = commands::commands_hash(&commands);
    
    let model = models::intent_classifier::load(models::registry(), "intent-classifier").await?;
    
    // check if we can use cached training data
    let config_dir = APP_CONFIG_DIR.get().ok_or("Config dir not set")?;
    let hash_path = config_dir.join(COMMANDS_HASH_FILE);
    let cache_path = config_dir.join(TRAINING_CACHE_FILE);
    
    let should_retrain = if hash_path.exists() && cache_path.exists() {
        let stored_hash = fs::read_to_string(&hash_path).unwrap_or_default();
        stored_hash.trim() != current_hash
    } else {
        true
    };
    
    if should_retrain {
        info!("Training intent classifier with {} commands...", commands.len());
        train_classifier(&model.classifier, &commands).await?;
        
        if let Ok(export) = model.classifier.export_training_data().await {
            let _ = fs::write(&cache_path, export);
            let _ = fs::write(&hash_path, &current_hash);
            info!("Training data cached.");
        }
    } else {
        info!("Loading cached training data...");
        if let Ok(data) = fs::read_to_string(&cache_path) {
            model.classifier.import_training_data(&data).await
                .map_err(|e| format!("Failed to import training data: {}", e))?;
        }
    }
    
    MODEL.set(model).map_err(|_| "Model already set")?;
    
    Ok(())
}

// re-train from a new command list. reuses the Arc already in MODEL; the
// OnceCell is never touched, so this can run any number of times.
//
// clear_training_data() first: train_classifier() only ever calls
// add_training_example(), so re-running it on top of the existing data would
// duplicate every example and skew confidences.
//
// ALL-OR-NOTHING. clear_training_data() empties training_data, vocabulary and
// intent_patterns and reloads only the upstream crate's bootstrap examples, and
// add_training_example() can reject an example mid-way. Without the snapshot
// below a single bad example would leave the classifier holding bootstrap plus
// a partial prefix of the commands - permanently, because nothing re-runs this.
pub async fn retrain(commands: &[JCommandsList]) -> Result<(), String> {
    let model = match MODEL.get() {
        Some(m) => m,
        None => {
            warn!("IntentClassifier not initialized, skipping retrain");
            return Ok(());
        }
    };

    // taken BEFORE the clear, and only used on the failure path
    let snapshot = model.classifier.export_training_data().await.ok();

    model.classifier.clear_training_data().await
        .map_err(|e| format!("Failed to clear training data: {}", e))?;

    if let Err(e) = train_classifier(&model.classifier, commands).await {
        restore_training_data(&model.classifier, snapshot).await;
        return Err(e);
    }

    // rewrite the cache pair only AFTER a clean retrain. a hash file claiming
    // to match a list it does not would make the next cold start load stale
    // training data and never notice.
    let config_dir = APP_CONFIG_DIR.get().ok_or("Config dir not set")?;
    if let Ok(export) = model.classifier.export_training_data().await {
        let _ = fs::write(config_dir.join(TRAINING_CACHE_FILE), export);
        let _ = fs::write(config_dir.join(COMMANDS_HASH_FILE),
                          commands::commands_hash(commands));
    }

    Ok(())
}

// put the pre-clear training data back after a failed retrain. best effort by
// definition - if this fails too there is nothing left to fall back to, so it
// is logged rather than propagated over the error that actually matters.
async fn restore_training_data(
    classifier: &intent_classifier::IntentClassifier,
    snapshot: Option<String>,
) {
    let snapshot = match snapshot {
        Some(s) => s,
        None => {
            error!("Retrain failed and no training snapshot was taken - \
                    intent recognition is degraded until restart");
            return;
        }
    };

    if let Err(e) = classifier.clear_training_data().await {
        error!("Failed to clear training data while rolling back a retrain: {}", e);
        return;
    }

    match classifier.import_training_data(&snapshot).await {
        Ok(()) => warn!("Retrain failed; previous training data restored"),
        Err(e) => error!("Failed to restore training data after a failed retrain: {}", e),
    }
}

pub async fn classify(text: &str) -> Result<IntentPrediction, IntentError> {
    let model = MODEL.get().expect("IntentClassifier not initialized");
    model.classifier.predict_intent(text).await
}

async fn train_classifier(
    classifier: &intent_classifier::IntentClassifier,
    commands: &[JCommandsList]
) -> Result<(), String> {
    let lang = i18n::get_language();
    info!("Training intent classifier for language: {}", lang);

    let mut total_examples = 0;
    let mut blank = 0;

    for assistant_cmd in commands {
        for cmd in &assistant_cmd.commands {
            let phrases = cmd.get_phrases(&lang);

            for phrase in phrases.iter() {
                // add_training_example() rejects an empty text outright, which
                // would abort the whole retrain over one stray blank line in a
                // hand-edited pack. degrade the example, not the model.
                if phrase.trim().is_empty() {
                    blank += 1;
                    continue;
                }

                let example = TrainingExample {
                    text: phrase.clone(),
                    intent: IntentId::from(cmd.id.as_str()),
                    confidence: 1.0,
                    source: TrainingSource::Programmatic,
                };
                
                classifier.add_training_example(example).await
                    .map_err(|e| format!("Failed to add training example: {}", e))?;
                
                total_examples += 1;
            }
        }
    }

    if blank > 0 {
        warn!("Skipped {} blank phrase(s) while training", blank);
    }

    info!("Added {} training examples for language '{}'", total_examples, lang);
    Ok(())
}
