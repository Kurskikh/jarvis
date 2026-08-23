mod registry;
mod catalog;
pub mod structs;
pub mod loaders;

pub mod vosk_models;
pub mod gliner_models;

// re-export loaders
#[cfg(feature = "jarvis_app")]
pub use loaders::embedding;

#[cfg(feature = "jarvis_app")]
pub use loaders::gliner;

#[cfg(feature = "jarvis_app")]
pub use loaders::ort_model;

#[cfg(feature = "jarvis_app")]
pub use loaders::intent_classifier;

#[cfg(feature = "vosk")]
pub use loaders::vosk;

#[cfg(feature = "nnnoiseless")]
pub use loaders::nnnoiseless;

pub use registry::ModelRegistry;
pub use structs::{Task, ModelDef, BackendOption};

use once_cell::sync::OnceCell;

use crate::APP_DIR;

pub const MODELS_PATH: &str = "resources/models";

static REGISTRY: OnceCell<ModelRegistry> = OnceCell::new();

pub fn init() -> Result<(), String> {
    if REGISTRY.get().is_some() {
        return Ok(());
    }

    let registry = ModelRegistry::new();

    let models_dir = APP_DIR.join(MODELS_PATH);
    let scan = catalog::scan_models(&models_dir);
    if scan.dir_available {
        info!("Found {} usable model(s) in {:?}", scan.models.len(), models_dir);
    } else {
        warn!(
            "No models directory at {:?}. Model ids in the settings will be left alone instead of being treated as invalid.",
            models_dir
        );
    }
    registry.set_catalog(scan.models, scan.dir_available);

    REGISTRY.set(registry)
        .map_err(|_| "Models registry already initialized".to_string())?;

    Ok(())
}

pub fn registry() -> &'static ModelRegistry {
    REGISTRY.get().expect("Models registry not initialized - call models::init() first")
}

// non-panicking accessor. None when init() has not run in this process
pub fn try_registry() -> Option<&'static ModelRegistry> {
    REGISTRY.get()
}

// available backend options for a task.
// returns an empty vec (and warns) when the registry is not initialized,
// so a missed init() degrades to an empty dropdown instead of a crash
pub fn get_options(task: Task) -> Vec<BackendOption> {
    match try_registry() {
        Some(reg) => reg.with_catalog(|models| catalog::get_options(task, models)),
        None => {
            warn!("models::get_options({:?}) called before models::init()", task);
            Vec::new()
        }
    }
}

// the id a task falls back to when the stored value cannot be used
pub fn default_backend(task: Task) -> &'static str {
    catalog::default_backend(task)
}

// tri-state validation, for write paths that run in processes which may or
// may not be able to judge the id.
//   Some(true)  -> id is valid for the task
//   Some(false) -> id is definitely NOT valid
//   None        -> no judgement possible; leave the value alone
//
// None is returned in two situations, and both are load bearing:
//   * the registry was never initialized (jarvis-cli)
//   * this install has no models directory at all - an unbundled build, or a
//     target dir that has not been synced. an absent directory is not evidence
//     that the user's model id is bogus, and Settings::sanitize_backends()
//     PERSISTS whatever it decides, so guessing wrong here silently destroys
//     the choice in the app.db both binaries share.
pub fn check_backend(task: Task, backend_id: &str) -> Option<bool> {
    let reg = try_registry()?;

    // "none" and the code backends exist no matter what is on disk
    if catalog::is_builtin_backend(task, backend_id) {
        return Some(true);
    }

    if !reg.catalog_available() {
        return None;
    }

    Some(reg.with_catalog(|models| catalog::is_valid_backend(task, backend_id, models)))
}
