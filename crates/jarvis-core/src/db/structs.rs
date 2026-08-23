use crate::config;
use serde::{Deserialize, Serialize};

use crate::config::structs::SpeechToTextEngine;
use crate::config::structs::WakeWordEngine;
use crate::config::structs::NoiseSuppressionBackend;

// an app.db written before Porcupine was dropped still names it. serde would
// fail the whole struct, and db::init_settings() falls back to Settings::default()
// on ANY parse error - one stale value would silently wipe every other setting.
// so this field degrades on its own instead of taking the file down with it.
fn deserialize_wake_word_engine<'de, D>(d: D) -> Result<WakeWordEngine, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = String::deserialize(d)?;
    Ok(match raw.as_str() {
        "Rustpotter" => WakeWordEngine::Rustpotter,
        "Vosk" => WakeWordEngine::Vosk,
        other => {
            warn!("Unknown wake word engine '{}' in settings, using the default.", other);
            config::DEFAULT_WAKE_WORD_ENGINE
        }
    })
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Settings {
    pub microphone: i32,
    pub voice: String,

    #[serde(deserialize_with = "deserialize_wake_word_engine")]
    pub wake_word_engine: WakeWordEngine,

    // backend selections (string IDs matching model or code backend IDs)
    #[serde(default = "default_intent_backend")]
    pub intent_backend: String,
    #[serde(default = "default_slots_backend")]
    pub slots_backend: String,
    #[serde(default = "default_vad_backend")]
    pub vad_backend: String,

    pub gliner_model: String,

    pub speech_to_text_engine: SpeechToTextEngine,
    pub vosk_model: String,

    // audio processing
    pub noise_suppression: NoiseSuppressionBackend,
    pub gain_normalizer: bool,

    #[serde(default = "default_language")]
    pub language: String,

    // absolute path to an AutoHotkey interpreter (AutoHotkey.exe / AutoHotkeyUX.exe)
    // or to an AutoHotkey install directory. empty = discover via the registry.
    #[serde(default)]
    pub ahk_interpreter: String,

    pub api_keys: ApiKeys,
}

fn default_intent_backend() -> String { config::DEFAULT_INTENT_BACKEND.to_string() }
fn default_slots_backend() -> String { config::DEFAULT_SLOTS_BACKEND.to_string() }
fn default_vad_backend() -> String { config::DEFAULT_VAD_BACKEND.to_string() }
fn default_language() -> String { crate::i18n::detect_system_language().to_string() }

// validate a backend id against the model registry.
// when the registry is not initialized in this process (jarvis-cli) we cannot
// judge, so the value is accepted as-is; Settings::sanitize_backends() then
// clamps it in whichever process does have a registry.
fn validated_backend(
    task: crate::models::Task,
    key: &str,
    val: &str,
) -> Result<String, String> {
    match crate::models::check_backend(task, val) {
        Some(false) => {
            let valid: Vec<String> = crate::models::get_options(task)
                .into_iter()
                .map(|o| o.id)
                .collect();
            Err(format!(
                "invalid value for '{}': '{}' (valid: {})",
                key,
                val,
                valid.join(", ")
            ))
        }
        _ => Ok(val.to_string()),
    }
}

// reset one backend field to the task's default if the registry says its
// current value is unknown. plain fn, not a closure, to avoid closure-lifetime
// unification across the three call sites.
//
// only Some(false) resets. check_backend() returns None when this process
// cannot judge - no registry (jarvis-cli) or no models directory at all - and
// the reset is persisted, so a guess here permanently overwrites a choice the
// user made in a build that COULD see the models.
fn fix_backend(
    key: &'static str,
    task: crate::models::Task,
    field: &mut String,
    fixed: &mut Vec<(&'static str, String, String)>,
) {
    if crate::models::check_backend(task, field.as_str()) == Some(false) {
        let default = crate::models::default_backend(task);
        fixed.push((key, field.clone(), default.to_string()));
        *field = default.to_string();
    }
}

// ### KEY-VALUE ACCESS

impl Settings {
    /// read a setting by key. returns None for unknown keys.
    pub fn get(&self, key: &str) -> Option<String> {
        match key {
            "selected_microphone"       => Some(self.microphone.to_string()),
            "assistant_voice"           => Some(self.voice.clone()),
            "selected_wake_word_engine" => Some(format!("{:?}", self.wake_word_engine)),
            "intent_backend"            => Some(self.intent_backend.clone()),
            "slots_backend"             => Some(self.slots_backend.clone()),
            "vad_backend"               => Some(self.vad_backend.clone()),
            "selected_gliner_model"     => Some(self.gliner_model.clone()),
            "selected_vosk_model"       => Some(self.vosk_model.clone()),
            "speech_to_text_engine"     => Some(format!("{:?}", self.speech_to_text_engine)),
            "noise_suppression"         => Some(format!("{:?}", self.noise_suppression)),
            "gain_normalizer"           => Some(self.gain_normalizer.to_string()),
            "language"                  => Some(self.language.clone()),
            "ahk_interpreter"           => Some(self.ahk_interpreter.clone()),
            "api_key__openai"           => Some(self.api_keys.openai.clone()),
            _ => None,
        }
    }

    /// write a setting by key. returns Err for unknown keys or invalid values.
    pub fn set(&mut self, key: &str, val: &str) -> Result<(), String> {
        match key {
            "selected_microphone" => {
                self.microphone = val.parse::<i32>()
                    .map_err(|_| format!("invalid integer: '{}'", val))?;
            }
            "assistant_voice" => {
                self.voice = val.to_string();
            }
            "selected_wake_word_engine" => {
                self.wake_word_engine = match val.to_lowercase().as_str() {
                    "rustpotter" => WakeWordEngine::Rustpotter,
                    "vosk"       => WakeWordEngine::Vosk,
                    _ => return Err(format!("unknown wake word engine: '{}'", val)),
                };
            }
            "intent_backend" => {
                self.intent_backend =
                    validated_backend(crate::models::Task::Intent, key, val)?;
            }
            "slots_backend" => {
                self.slots_backend =
                    validated_backend(crate::models::Task::Slots, key, val)?;
            }
            "vad_backend" => {
                self.vad_backend =
                    validated_backend(crate::models::Task::Vad, key, val)?;
            }
            "selected_gliner_model" => {
                self.gliner_model = val.to_string();
            }
            "selected_vosk_model" => {
                self.vosk_model = val.to_string();
            }
            "noise_suppression" => {
                self.noise_suppression = match val.to_lowercase().as_str() {
                    "none"        => NoiseSuppressionBackend::None,
                    "nnnoiseless" => NoiseSuppressionBackend::Nnnoiseless,
                    _ => return Err(format!("unknown noise suppression backend: '{}'", val)),
                };
            }
            "gain_normalizer" => {
                self.gain_normalizer = match val.to_lowercase().as_str() {
                    "true"  => true,
                    "false" => false,
                    _ => return Err(format!("expected 'true' or 'false', got: '{}'", val)),
                };
            }
            "language" => {
                self.language = val.to_string();
            }
            "ahk_interpreter" => {
                let path = val.trim();
                if !path.is_empty() && !std::path::Path::new(path).exists() {
                    return Err(format!("path does not exist: '{}'", path));
                }
                self.ahk_interpreter = path.to_string();
            }
            "api_key__openai" => {
                self.api_keys.openai = val.to_string();
            }
            _ => return Err(format!("unknown setting: '{}'", key)),
        }
        Ok(())
    }

    /// clamp backend selections to ids the model registry actually knows.
    /// no-op when this process cannot judge: registry not initialized, or no
    /// models directory next to the executable (see models::check_backend).
    /// returns (key, old_value, new_value) for every field that was reset.
    pub fn sanitize_backends(&mut self) -> Vec<(&'static str, String, String)> {
        use crate::models::Task;

        let mut fixed: Vec<(&'static str, String, String)> = Vec::new();

        fix_backend("intent_backend", Task::Intent, &mut self.intent_backend, &mut fixed);
        fix_backend("slots_backend", Task::Slots, &mut self.slots_backend, &mut fixed);
        fix_backend("vad_backend", Task::Vad, &mut self.vad_backend, &mut fixed);

        fixed
    }

    /// all valid setting keys (for enumeration, debugging, etc.)
    pub fn keys() -> &'static [&'static str] {
        &[
            "selected_microphone",
            "assistant_voice",
            "selected_wake_word_engine",
            "intent_backend",
            "slots_backend",
            "vad_backend",
            "selected_gliner_model",
            "selected_vosk_model",
            "speech_to_text_engine",
            "noise_suppression",
            "gain_normalizer",
            "language",
            "ahk_interpreter",
            "api_key__openai",
        ]
    }
}

// ### DEFAULT

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            microphone: -1,
            voice: String::from(""),

            wake_word_engine: config::DEFAULT_WAKE_WORD_ENGINE,

            intent_backend: config::DEFAULT_INTENT_BACKEND.to_string(),
            slots_backend: config::DEFAULT_SLOTS_BACKEND.to_string(),
            vad_backend: config::DEFAULT_VAD_BACKEND.to_string(),

            gliner_model: String::new(),
            speech_to_text_engine: config::DEFAULT_SPEECH_TO_TEXT_ENGINE,
            vosk_model: String::from(""),

            noise_suppression: config::DEFAULT_NOISE_SUPPRESSION,
            gain_normalizer: config::DEFAULT_GAIN_NORMALIZER,

            language: crate::i18n::detect_system_language().to_string(),

            ahk_interpreter: String::new(),

            api_keys: ApiKeys {
                openai: String::from(""),
            },
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ApiKeys {
    pub openai: String,
}
