use serde::Serialize;

use jarvis_core::commands::JCommand;
use jarvis_core::reload;

use crate::commands_editor::{self, PackFiles, PackOnDisk};

// hard errors and soft warnings kept apart. chaining them into one Vec made the
// page render "this command is broken at runtime" under an orange "Warnings"
// heading next to a teal "saved" banner.
#[derive(Serialize, Debug, Default)]
pub struct PackValidation {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

// the GUI reads its own list back immediately after a write. without this the
// editor would keep serving its own pre-edit state, and get_commands_count()
// would go stale mid-session.
fn refresh_local(context: &str) {
    if let Err(e) = reload::reload_list() {
        log::warn!("{}: local list refresh failed: {}", context, e);
    }
}

#[tauri::command]
pub fn list_command_packs() -> Result<Vec<PackOnDisk>, String> {
    commands_editor::list_packs()
}

#[tauri::command]
pub fn read_command_pack(pack: String) -> Result<PackOnDisk, String> {
    commands_editor::read_pack(&pack)
}

#[tauri::command]
pub fn read_command_pack_raw(pack: String) -> Result<String, String> {
    commands_editor::read_pack_raw(&pack)
}

// `revision` is the one read_command_pack handed out. it is what stops a save
// from silently overwriting a hand edit made in the same folder the user is
// told to open for the .lua/.ahk bodies.
#[tauri::command]
pub fn save_command_pack(
    pack: String,
    commands: Vec<JCommand>,
    revision: Option<String>,
) -> Result<PackOnDisk, String> {
    commands_editor::write_pack(&pack, &commands, revision.as_deref())?;
    refresh_local(&format!("save_command_pack('{}')", pack));

    commands_editor::read_pack(&pack)
}

#[tauri::command]
pub fn save_command_pack_raw(
    pack: String,
    content: String,
    revision: Option<String>,
) -> Result<PackOnDisk, String> {
    commands_editor::write_pack_raw(&pack, &content, revision.as_deref())?;
    refresh_local(&format!("save_command_pack_raw('{}')", pack));

    commands_editor::read_pack(&pack)
}

#[tauri::command]
pub fn create_command_pack(pack: String) -> Result<PackOnDisk, String> {
    commands_editor::create_pack(&pack)?;
    refresh_local(&format!("create_command_pack('{}')", pack));

    commands_editor::read_pack(&pack)
}

// `confirm` must repeat the pack name. that is the API-level half of "must not
// be possible by accident from a stray click"; the modal is the UI-level half.
#[tauri::command]
pub fn delete_command_pack(pack: String, confirm: String) -> Result<(), String> {
    if confirm != pack {
        return Err(format!("Refusing to delete '{}': confirmation does not match", pack));
    }

    let moved = commands_editor::trash_pack(&pack)?;
    log::info!("Command pack '{}' moved to {}", pack, moved.display());

    refresh_local(&format!("delete_command_pack('{}')", pack));

    Ok(())
}

// infallible by design (the list_backend_options precedent): a validation
// hiccup must never be able to stop the editor from opening a pack.
#[tauri::command]
pub fn validate_command_pack(pack: String, commands: Vec<JCommand>) -> PackValidation {
    PackValidation {
        errors: commands_editor::validate_pack(&pack, &commands).err().into_iter().collect(),
        warnings: commands_editor::validate_pack_warnings(&pack, &commands),
    }
}

#[tauri::command]
pub fn list_pack_files(pack: String) -> Result<PackFiles, String> {
    commands_editor::list_pack_files(&pack)
}

#[tauri::command]
pub fn list_sound_names(voice_id: String, lang: String) -> Vec<String> {
    commands_editor::list_sound_names(&voice_id, &lang)
}

#[tauri::command]
pub fn get_command_types() -> Vec<&'static str> {
    commands_editor::COMMAND_TYPES.to_vec()
}

#[tauri::command]
pub fn get_sandbox_levels() -> Vec<&'static str> {
    commands_editor::SANDBOX_LEVELS.to_vec()
}

#[tauri::command]
pub fn get_default_timeout() -> u64 {
    commands_editor::DEFAULT_TIMEOUT_MS
}
