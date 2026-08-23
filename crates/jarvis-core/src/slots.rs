mod gliner;

use std::collections::HashMap;
use once_cell::sync::OnceCell;

use crate::commands::{SlotDefinition, SlotValue};
use crate::{models, DB};

static BACKEND: OnceCell<String> = OnceCell::new();

pub fn init() -> Result<(), String> {
    if BACKEND.get().is_some() {
        return Ok(());
    }

    let requested = DB.get()
        .map(|db| db.read().slots_backend.clone())
        .unwrap_or_else(|| "none".to_string());

    // BACKEND is set from the outcome, not from the request: if the model does
    // not load, extract() must dispatch to the disabled arm instead of calling
    // into an uninitialized GLiNER on every single utterance
    let attempt = init_backend(&requested);

    let effective = match attempt {
        Ok(()) => requested,
        Err(e) => {
            error!("Slot backend '{}' failed to initialize: {}. Slot extraction disabled.", requested, e);
            "none".to_string()
        }
    };

    BACKEND.set(effective).map_err(|_| "Slot backend already set")?;

    Ok(())
}

fn init_backend(backend: &str) -> Result<(), String> {
    match backend {
        "none" => {
            info!("Slot extraction disabled");
        }
        // any model ID is treated as a GLiNER model for now
        model_id => {
            info!("Initializing GLiNER slot extraction with model '{}'.", model_id);
            // try_registry(), not registry(): a missed models::init() must be a
            // recoverable error, not a panic
            let registry = models::try_registry()
                .ok_or_else(|| "model registry is not initialized".to_string())?;
            let model = models::gliner::load(registry, model_id)?;
            gliner::init_with_model(model)?;
            info!("GLiNER slot extraction initialized.");
        }
    }

    Ok(())
}

pub fn extract(
    text: &str,
    slots: &HashMap<String, SlotDefinition>,
) -> HashMap<String, SlotValue> {
    if slots.is_empty() {
        return HashMap::new();
    }

    match BACKEND.get().map(|s| s.as_str()).unwrap_or("none") {
        "none" => HashMap::new(),
        _ => {
            match gliner::extract(text, slots) {
                Ok(result) => result,
                Err(e) => {
                    error!("GLiNER slot extraction failed: {}", e);
                    HashMap::new()
                }
            }
        }
    }
}
