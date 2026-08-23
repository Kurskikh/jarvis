pub mod structs;
pub mod manager;

use crate::{config, APP_CONFIG_DIR};

use log::info;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

pub use manager::SettingsManager;

fn get_db_file_path() -> PathBuf {
    PathBuf::from(format!(
        "{}/{}",
        APP_CONFIG_DIR.get().unwrap().display(),
        config::DB_FILE_NAME
    ))
}

pub fn init_settings() -> structs::Settings {
    let db_file_path = get_db_file_path();

    info!(
        "Loading settings db file located at: {}",
        db_file_path.display()
    );

    if db_file_path.exists() {
        if let Ok(db_file) = File::open(&db_file_path) {
            let reader = BufReader::new(db_file);
            if let Ok(settings) = serde_json::from_reader(reader) {
                info!("Settings loaded.");
                return settings;
            }
        }
    }

    warn!("No settings file found or there was an error parsing it. Creating default struct.");
    structs::Settings::default()
}

/// init settings and return a SettingsManager ready to use
pub fn init() -> SettingsManager {
    let settings = init_settings();
    SettingsManager::new(settings)
}

/// re-read app.db and adopt the LLM fields into the live settings of THIS
/// process. returns true when something actually changed.
///
/// jarvis-app loads settings once at startup and jarvis-gui is a separate
/// process: without this, an llm_* value saved in the settings window never
/// reaches the running assistant, and every "set it in Settings and try again"
/// in llm::error is a lie. the GUI fires IpcAction::ReloadSettings after a
/// successful save and this is what answers it.
///
/// ONLY the llm_* keys and the openai token are adopted. a blanket swap would
/// silently change values that were consumed at init - microphone index, stt
/// model, wake-word engine - leaving the live struct describing a configuration
/// the process is not actually running. those still need a restart, and saying
/// so is honest; the LLM fields are read fresh on every turn, so they do not.
pub fn reload_llm_settings() -> Result<bool, String> {
    let db = crate::DB.get().ok_or("settings are not initialized in this process")?;

    let db_file_path = get_db_file_path();
    let file = File::open(&db_file_path)
        .map_err(|e| format!("cannot open {}: {}", db_file_path.display(), e))?;
    let on_disk: structs::Settings = serde_json::from_reader(BufReader::new(file))
        .map_err(|e| format!("cannot parse {}: {}", db_file_path.display(), e))?;

    let mut live = db.write();

    let changed = live.llm_enabled != on_disk.llm_enabled
        || live.llm_base_url != on_disk.llm_base_url
        || live.llm_model != on_disk.llm_model
        || live.llm_timeout != on_disk.llm_timeout
        || live.llm_system_prompt != on_disk.llm_system_prompt
        || live.llm_allow_remote != on_disk.llm_allow_remote
        || live.api_keys.openai != on_disk.api_keys.openai;

    live.llm_enabled = on_disk.llm_enabled;
    live.llm_base_url = on_disk.llm_base_url;
    live.llm_model = on_disk.llm_model;
    live.llm_timeout = on_disk.llm_timeout;
    live.llm_system_prompt = on_disk.llm_system_prompt;
    live.llm_allow_remote = on_disk.llm_allow_remote;
    live.api_keys.openai = on_disk.api_keys.openai;

    Ok(changed)
}

pub fn save_settings(settings: &structs::Settings) -> Result<(), std::io::Error> {
    let db_file_path = get_db_file_path();

    std::fs::write(
        &db_file_path,
        serde_json::to_string_pretty(&settings).unwrap(),
    )?;

    info!("Settings saved to: {:#}", db_file_path.display());
    Ok(())
}
