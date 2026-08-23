use jarvis_core::commands::JCommand;

// both of these read the process-shared snapshot, never a Lazy of their own:
// every editor write republishes it through reload::reload_list(), so the count
// in the header cannot drift away from what is actually on disk.

#[tauri::command]
pub fn get_commands_count() -> usize {
    jarvis_core::commands_list()
        .iter()
        .map(|list| list.commands.len())
        .sum()
}

#[tauri::command]
pub fn get_commands_list() -> Vec<JCommand> {
    jarvis_core::commands_list()
        .iter()
        .flat_map(|list| list.commands.clone())
        .collect()
}
