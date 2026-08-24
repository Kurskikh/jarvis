// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use jarvis_core::{config, db, i18n, models, voices, DB, SettingsManager};

#[macro_use]
extern crate simple_log;

mod events;

mod commands_editor;

mod tauri_commands;

#[derive(Clone)]
pub struct AppState {
    pub settings: SettingsManager,
}

fn main() {
    config::init_dirs().expect("Failed to init dirs");
    
    // basic logging setup (simpler for GUI)
    simple_log::quick!("info");

    // init settings
    let manager = db::init();

    // init models registry (scans resources/models for model.toml descriptors).
    // must run before any command that touches models::get_options
    if let Err(e) = models::init() {
        warn!("Models registry init failed: {}", e);
    }

    // clamp any backend setting the registry does not recognise (old/hand-edited
    // app.db). runs after models::init(), before the settings page can read them
    manager.sanitize_backends();

    // init i18n
    i18n::init(&manager.lock().language);

    // init voices
    if let Err(e) = voices::init(&manager.lock().voice, &manager.lock().language) {
        eprintln!("Failed to init voices: {}", e);
    }

    // init audio backend
    if let Err(e) = jarvis_core::audio::init() {
        eprintln!("Failed to init audio: {:?}", e);
    }

    // set global DB (for core modules that read settings at init time)
    DB.set(manager.arc().clone())
            .expect("DB already initialized");

    // load the command list into the process-shared snapshot. must run after
    // i18n::init (reload_list hashes the current-language phrases). every
    // editor write republishes it, so get_commands_count() never goes stale
    if let Err(e) = jarvis_core::reload::reload_list() {
        warn!("Failed to load commands: {}", e);
    }

    tauri::Builder::default()
        .manage(AppState { settings: manager })
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            // audio
            tauri_commands::pv_get_audio_devices,
            tauri_commands::pv_get_audio_device_name,
            tauri_commands::play_sound,

            // db
            tauri_commands::db_read,
            tauri_commands::db_write,
            tauri_commands::db_write_many,

            // etc
            tauri_commands::get_app_version,

            // fs
            tauri_commands::get_log_file_path,
            tauri_commands::show_in_folder,

            // sys
            tauri_commands::get_current_ram_usage,
            tauri_commands::get_peak_ram_usage,
            tauri_commands::get_cpu_temp,
            tauri_commands::get_cpu_usage,
            tauri_commands::get_jarvis_app_stats,
            tauri_commands::is_jarvis_app_running,
            tauri_commands::run_jarvis_app,

            // vosk
            tauri_commands::list_vosk_models,

            // gliner
            tauri_commands::list_gliner_models,

            // model registry
            tauri_commands::list_backend_options,
            tauri_commands::list_llm_models,
            tauri_commands::check_speech_sidecar,

            // i18n
            tauri_commands::get_translations,
            tauri_commands::translate,
            tauri_commands::get_current_language,
            tauri_commands::set_language,
            tauri_commands::get_supported_languages,

            // commands
            tauri_commands::get_commands_count,
            tauri_commands::get_commands_list,

            // command editor
            tauri_commands::list_command_packs,
            tauri_commands::read_command_pack,
            tauri_commands::save_command_pack,
            tauri_commands::create_command_pack,
            tauri_commands::delete_command_pack,
            tauri_commands::validate_command_pack,
            tauri_commands::read_command_pack_raw,
            tauri_commands::save_command_pack_raw,
            tauri_commands::list_pack_files,
            tauri_commands::list_sound_names,
            tauri_commands::get_command_types,
            tauri_commands::get_sandbox_levels,
            tauri_commands::get_default_timeout,

            // voices
            tauri_commands::list_voices,
            tauri_commands::get_voice,
            tauri_commands::preview_voice,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
