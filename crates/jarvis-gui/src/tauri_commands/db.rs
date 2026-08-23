use crate::AppState;

#[tauri::command]
pub fn db_read(state: tauri::State<'_, AppState>, key: &str) -> String {
    match state.settings.read(key) {
        Some(val) => val,
        None => {
            // unknown key => Settings::get returned None. previously this was
            // .unwrap_or_default(), which made a drifted key name invisible
            log::warn!("db_read('{}'): unknown setting key", key);
            String::new()
        }
    }
}

#[tauri::command]
pub fn db_write(state: tauri::State<'_, AppState>, key: &str, val: &str) -> Result<(), String> {
    state.settings.write(key, val).map_err(|e| {
        log::warn!("db_write('{}', '{}'): {}", key, val, e);
        e
    })
}

// write a whole form in one shot.
//
// the settings page used to fire one db_write per field. every one of those
// serializes the entire Settings struct and rewrites app.db, and a rejected
// value only aborted its own invoke - the other fields were already on disk,
// so the user saw "Error" over a save that had mostly gone through.
// write_many() validates every pair before touching anything, so the save
// either lands whole or not at all, and app.db is written exactly once.
#[tauri::command]
pub fn db_write_many(
    state: tauri::State<'_, AppState>,
    entries: std::collections::HashMap<String, String>,
) -> Result<(), String> {
    let pairs: Vec<(&str, &str)> = entries
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    state.settings.write_many(&pairs).map_err(|e| {
        log::warn!("db_write_many({} entries): {}", pairs.len(), e);
        e
    })
}
