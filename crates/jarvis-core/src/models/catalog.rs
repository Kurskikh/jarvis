use std::fs;
use std::path::Path;

use crate::config;
use super::structs::{Task, ModelDef, BackendOption};

// result of a models-directory scan.
//
// `dir_available` distinguishes "the models directory was there and we read it"
// from "there is no models directory in this install". an empty `models` vec
// alone cannot tell those apart, and treating the second case as authoritative
// makes validation reset perfectly good settings (see models::check_backend).
pub struct ScanResult {
    pub models: Vec<ModelDef>,
    pub dir_available: bool,
}

// scan the models directory for folders containing model.toml
pub fn scan_models(models_dir: &Path) -> ScanResult {
    let mut models = Vec::new();

    if !models_dir.exists() {
        warn!("Models directory not found: {:?}", models_dir);
        return ScanResult { models, dir_available: false };
    }

    let entries = match fs::read_dir(models_dir) {
        Ok(e) => e,
        Err(e) => {
            warn!("Failed to read models dir: {}", e);
            return ScanResult { models, dir_available: false };
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let toml_path = path.join("model.toml");
        if !toml_path.exists() {
            continue;
        }

        match load_model_def(&toml_path, &path) {
            Ok(mut def) => {
                // a descriptor is not a model. model.toml is the only file in
                // models/app/catalog that is version controlled, so a fresh clone
                // (or a partial download) has descriptors whose weights are
                // missing. keep only the tasks whose loader can actually run,
                // otherwise the id would be offered in the UI, pass validation,
                // and then fail at load time.
                let declared = def.tasks.len();
                def.tasks.retain(|task| task_files_present(*task, &path));

                if def.tasks.is_empty() {
                    warn!(
                        "Skipping model '{}' ({:?}): none of its declared tasks have \
                         their required files on disk",
                        def.id, path
                    );
                    continue;
                }

                if def.tasks.len() != declared {
                    warn!(
                        "Model '{}': some declared tasks are missing their files and \
                         were dropped, keeping {:?}",
                        def.id, def.tasks
                    );
                }

                info!("Found model: {} ({}) - tasks: {:?}", def.name, def.id, def.tasks);
                models.push(def);
            }
            Err(e) => warn!("Failed to load model from {:?}: {}", path, e),
        }
    }

    ScanResult { models, dir_available: true }
}

// files each loader reads, relative to the model directory.
// KEEP IN SYNC with the loader each arm names - models/loaders/*.rs for
// intent and slots, stt/tone.rs for stt, audio_processing/vad/silero.rs for
// vad. This is what makes catalog membership imply loadability.
fn task_files_present(task: Task, model_dir: &Path) -> bool {
    match task {
        // loaders/embedding.rs
        Task::Intent => [
            "model.onnx",
            "tokenizer.json",
            "config.json",
            "special_tokens_map.json",
            "tokenizer_config.json",
        ]
        .iter()
        .all(|f| model_dir.join(f).is_file()),

        // loaders/gliner.rs - onnx/model.onnx when the subfolder exists,
        // model.onnx otherwise, plus the tokenizer
        Task::Slots => {
            model_dir.join("tokenizer.json").is_file()
                && (model_dir.join("onnx").join("model.onnx").is_file()
                    || model_dir.join("model.onnx").is_file())
        }

        // stt/tone.rs hands both files straight to sherpa-onnx. Checked here
        // so a model whose weights have not been downloaded is simply not
        // offered, the same as every other task - the alternative is a choice
        // in the dropdown that fails at startup.
        Task::Stt => {
            model_dir.join("model.onnx").is_file() && model_dir.join("tokens.txt").is_file()
        }

        // audio_processing/vad/silero.rs hands this file to sherpa-onnx
        Task::Vad => model_dir.join("model.onnx").is_file(),

        // no descriptor-driven backends for this yet; nothing to verify
        Task::NoiseSuppression => true,
    }
}

fn load_model_def(toml_path: &Path, model_dir: &Path) -> Result<ModelDef, String> {
    let content = fs::read_to_string(toml_path)
        .map_err(|e| format!("read error: {}", e))?;

    let parsed: ModelToml = toml::from_str(&content)
        .map_err(|e| format!("parse error: {}", e))?;

    let mut def = parsed.model;
    def.path = model_dir.to_path_buf();

    Ok(def)
}

#[derive(serde::Deserialize)]
struct ModelToml {
    model: ModelDef,
}

// Code backends per task
pub fn code_backends(task: Task) -> Vec<BackendOption> {
    match task {
        Task::Intent => vec![
            BackendOption {
                id: "intent-classifier".into(),
                name: "Intent Classifier".into(),
                model_id: None,
                is_default: false,
            },
        ],
        Task::Slots => vec![],
        Task::Vad => vec![
            BackendOption {
                id: "energy".into(),
                name: "Energy-based".into(),
                model_id: None,
                is_default: false,
            },
            // NOTE: deliberately NOT #[cfg(feature = "nnnoiseless")].
            // this list is rendered by jarvis-gui, which links jarvis-core with
            // default-features = false, while the process that actually runs VAD
            // is jarvis-app, which enables the feature. gating here would hide a
            // backend the running app supports.
            BackendOption {
                id: "nnnoiseless".into(),
                name: "Nnnoiseless".into(),
                model_id: None,
                is_default: false,
            },
        ],
        Task::NoiseSuppression => vec![
            BackendOption {
                id: "nnnoiseless".into(),
                name: "Nnnoiseless".into(),
                model_id: None,
                is_default: false,
            },
        ],
        Task::Stt => vec![
            BackendOption {
                id: "vosk".into(),
                name: "Vosk".into(),
                model_id: None,
                is_default: false,
            },
            // T-one is NOT listed here. It comes from its descriptor in the
            // catalogue below, which is what makes it appear only once its
            // weights are on disk. Listing it in both places produced two
            // entries with different ids - "t-one" here, "t-one-ru" there -
            // of which only one could be saved.
        ],
    }
}

// the id a task falls back to when the stored value cannot be used.
// single source of truth for both the Rust clamp (Settings::sanitize_backends)
// and the frontend clamp, which reads it off BackendOption::is_default.
pub fn default_backend(task: Task) -> &'static str {
    match task {
        Task::Intent => config::DEFAULT_INTENT_BACKEND,
        Task::Slots => config::DEFAULT_SLOTS_BACKEND,
        Task::Vad => config::DEFAULT_VAD_BACKEND,
        // these two are stored as enums, not backend ids; their defaults are
        // NoiseSuppressionBackend::None / SpeechToTextEngine::Vosk
        Task::NoiseSuppression => "none",
        Task::Stt => "vosk",
    }
}

// "none" and the code backends exist regardless of what is on disk
pub fn is_builtin_backend(task: Task, backend_id: &str) -> bool {
    backend_id == "none" || code_backends(task).iter().any(|b| b.id == backend_id)
}

// get all available options for a task:
// "none" first, then code backends, then AI models from catalog
pub fn get_options(task: Task, models: &[ModelDef]) -> Vec<BackendOption> {
    // "Disabled" is a real answer for noise suppression, slots or a VAD. It is
    // not one for speech recognition: an assistant that cannot transcribe a
    // command is not configured, it is broken.
    let mut options = if task == Task::Stt {
        Vec::new()
    } else {
        vec![
            BackendOption {
                id: "none".into(),
                name: "Disabled".into(),
                model_id: None,
                is_default: false,
            },
        ]
    };

    options.extend(code_backends(task));

    for model in models {
        if model.tasks.contains(&task) {
            options.push(BackendOption {
                id: model.id.clone(),
                name: model.name.clone(),
                model_id: Some(model.id.clone()),
                is_default: false,
            });
        }
    }

    let default_id = default_backend(task);
    for option in options.iter_mut() {
        option.is_default = option.id == default_id;
    }

    options
}

pub fn is_valid_backend(task: Task, backend_id: &str, models: &[ModelDef]) -> bool {
    if is_builtin_backend(task, backend_id) {
        return true;
    }

    models.iter().any(|m| m.id == backend_id && m.tasks.contains(&task))
}

#[cfg(test)]
mod vad_descriptor_tests {
    use super::{task_files_present, Task};

    fn empty_model_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("jarvis-catalog-tests").join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // A descriptor is not a model. Until this check existed for VAD, a
    // silero-vad folder holding nothing but model.toml was offered in the
    // dropdown, passed validation, and then failed at startup - the exact
    // failure mode the Stt arm above was written to prevent.
    #[test]
    fn a_vad_model_without_its_weights_is_not_offered() {
        let dir = empty_model_dir("vad-no-weights");
        assert!(!task_files_present(Task::Vad, &dir));
    }

    #[test]
    fn a_vad_model_with_its_weights_is_offered() {
        let dir = empty_model_dir("vad-with-weights");
        std::fs::write(dir.join("model.onnx"), b"not really a model").unwrap();
        assert!(task_files_present(Task::Vad, &dir));
    }

    // Noise suppression still has no descriptor-driven backends; a descriptor
    // declaring it must keep working exactly as before.
    #[test]
    fn noise_suppression_stays_unverified() {
        let dir = empty_model_dir("ns-nothing");
        assert!(task_files_present(Task::NoiseSuppression, &dir));
    }
}
