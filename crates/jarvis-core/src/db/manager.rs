use std::sync::Arc;
use parking_lot::RwLock;

use super::structs::Settings;
use super::save_settings;

// centralized settings manager.
// wraps Arc<RwLock<Settings>> and handles locking + auto-save
// can be used anywhere, ex. from GUI, tray, IPC, CLI, etc.
#[derive(Clone)]
pub struct SettingsManager {
    inner: Arc<RwLock<Settings>>,
}

impl SettingsManager {
    pub fn new(settings: Settings) -> Self {
        Self {
            inner: Arc::new(RwLock::new(settings)),
        }
    }

    // wrap an existing Arc<RwLock<Settings>>
    pub fn from_arc(arc: Arc<RwLock<Settings>>) -> Self {
        Self { inner: arc }
    }

    // read a setting by key
    pub fn read(&self, key: &str) -> Option<String> {
        self.inner.read().get(key)
    }

    // write a setting by key, auto-saves to disk
    pub fn write(&self, key: &str, val: &str) -> Result<(), String> {
        let snapshot = {
            // staged on a copy, same as write_many: an Err from set() or from
            // the cross-field validate() drops the guard with the live
            // settings untouched
            let mut settings = self.inner.write();
            let mut staged = settings.clone();
            staged.set(key, val)?;
            staged.validate_change(&settings)?;
            *settings = staged.clone();
            staged
        };

        save_settings(&snapshot)
            .map_err(|e| format!("failed to save settings: {}", e))?;

        Ok(())
    }

    // write multiple settings at once, single save.
    // all-or-nothing: every pair is applied to a staged copy first, so a
    // rejected key leaves both the live settings and the file untouched and a
    // form save cannot half-apply.
    pub fn write_many(&self, pairs: &[(&str, &str)]) -> Result<(), String> {
        let snapshot = {
            // stage on a copy while holding the write lock: an Err drops the
            // guard with the live settings untouched
            let mut settings = self.inner.write();
            let mut staged = settings.clone();
            for (key, val) in pairs {
                staged.set(key, val)
                    .map_err(|e| format!("'{}': {}", key, e))?;
            }
            // cross-field invariants, after every pair landed - order-independent
            staged.validate_change(&settings)?;
            *settings = staged.clone();
            staged
        };

        save_settings(&snapshot)
            .map_err(|e| format!("failed to save settings: {}", e))?;

        Ok(())
    }

    // clamp backend selections against the model registry and persist any
    // correction. call AFTER models::init(); no-op when the registry is absent
    pub fn sanitize_backends(&self) {
        let (fixed, snapshot) = {
            let mut settings = self.inner.write();
            let fixed = settings.sanitize_backends();
            (fixed, settings.clone())
        };

        if fixed.is_empty() {
            return;
        }

        for (key, old, new) in &fixed {
            warn!("Setting '{}' had unknown value '{}', reset to '{}'", key, old, new);
        }

        if let Err(e) = save_settings(&snapshot) {
            warn!("Failed to persist sanitized settings: {}", e);
        }
    }

    // direct read access to the full Settings struct (for init code that
    // needs to read multiple fields at once without key-based access)
    pub fn lock(&self) -> parking_lot::RwLockReadGuard<'_, Settings> {
        self.inner.read()
    }

    // direct write access (for bulk operations not covered by set())
    pub fn lock_mut(&self) -> parking_lot::RwLockWriteGuard<'_, Settings> {
        self.inner.write()
    }

    // get the underlying Arc
    pub fn arc(&self) -> &Arc<RwLock<Settings>> {
        &self.inner
    }

    // dump all settings as key-value pairs (for debugging)
    pub fn dump(&self) -> Vec<(String, String)> {
        let settings = self.inner.read();
        Settings::keys().iter()
            .filter_map(|&key| {
                settings.get(key).map(|val| (key.to_string(), val))
            })
            .collect()
    }
}
