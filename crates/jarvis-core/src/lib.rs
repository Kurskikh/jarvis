use once_cell::sync::{Lazy, OnceCell};
use parking_lot::RwLock;
use std::{sync::Arc};
use platform_dirs::AppDirs;
use std::path::PathBuf;

#[macro_use]
extern crate log;

pub mod time;

pub mod audio;
pub mod commands;
pub mod config;
pub mod db;
pub mod i18n;

#[cfg(feature = "jarvis_app")]
pub mod listener;

pub mod recorder;

pub mod reload;

#[cfg(feature = "jarvis_app")]
pub mod stt;

#[cfg(feature = "intent")]
pub mod intent;

#[cfg(feature = "jarvis_app")]
pub mod slots;

pub mod models;

// re-exported from models/
pub use models::vosk_models;
pub use models::gliner_models;

#[cfg(feature = "jarvis_app")]
pub mod audio_processing;

#[cfg(feature = "jarvis_app")]
pub mod ipc;

pub mod voices;

pub mod audio_buffer;

#[cfg(feature = "lua")]
pub mod lua;

#[cfg(feature = "llm")]
pub mod llm;

// speaking those answers out loud. Same feature gate: it exists to give the
// language model a voice, and it needs the same reqwest and tokio.
#[cfg(feature = "llm")]
pub mod speech;

// shared statics
// pub static APP_DIR: Lazy<PathBuf> = Lazy::new(|| std::env::current_dir().unwrap());
pub static APP_DIR: Lazy<PathBuf> = Lazy::new(|| {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap())
});
pub static SOUND_DIR: Lazy<PathBuf> = Lazy::new(|| APP_DIR.clone().join(config::SOUND_PATH));
pub static APP_DIRS: OnceCell<AppDirs> = OnceCell::new();
pub static APP_CONFIG_DIR: OnceCell<PathBuf> = OnceCell::new();
pub static APP_LOG_DIR: OnceCell<PathBuf> = OnceCell::new();
pub static DB: OnceCell<Arc<RwLock<db::structs::Settings>>> = OnceCell::new();
// the audio thread borrows &JCommand out of this list and holds that borrow
// for the whole command execution - up to cmd_config.timeout ms of Lua, or the
// 2s sleep of the "terminate" type. a read guard held that long would make a
// reload writer wait, and parking_lot's writer preference would then block the
// audio thread's NEXT read. so readers take an Arc snapshot instead: the guard
// lives just long enough to bump a refcount, and the old Vec stays alive until
// the last snapshot drops it.
static COMMANDS_LIST: Lazy<RwLock<Arc<Vec<JCommandsList>>>> =
    Lazy::new(|| RwLock::new(Arc::new(Vec::new())));

// snapshot of the live command list
pub fn commands_list() -> Arc<Vec<JCommandsList>> {
    COMMANDS_LIST.read().clone()
}

// publish a new command list. never waits on a reader.
pub fn set_commands_list(commands: Vec<JCommandsList>) {
    *COMMANDS_LIST.write() = Arc::new(commands);
}

// re-exports
pub use commands::JCommandsList;
pub use config::structs::*;
pub use db::structs::Settings;
pub use db::SettingsManager;

// use crate::commands::{JComandsList, JCommand};