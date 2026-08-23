#[cfg(feature = "intent")]
use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::commands;

// serializes parse+swap. the section holds no lock across I/O and has no
// awaits; it exists only so two rapid saves cannot interleave two parses.
static SWAP_GATE: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Default)]
pub struct ReloadReport {
    pub packs: usize,
    pub commands: usize,
    // the CURRENT-LANGUAGE phrase set changed, so the intent backend is stale
    pub phrases_changed: bool,
    // an intent backend was actually re-trained
    pub retrained: bool,
    // packs whose command.toml is on disk but does not parse. they are NOT in
    // the published list, so a caller that ignores this reports a reload as
    // clean while every command in those packs has silently disappeared.
    pub skipped: Vec<String>,
    // the list IS live, but the classifier could not be retrained on it. this
    // is not an Err: the swap already happened and rolling it back would be a
    // second lie. the assistant runs the new commands with stale intents.
    pub retrain_error: Option<String>,
}

// re-read resources/commands from disk and publish the result.
//
// Err means NOTHING was published: parse_commands_detailed() only fails when
// the directory itself cannot be read, and the swap is the last statement.
// a readable but empty directory publishes an empty list on purpose - after the
// last pack is deleted the assistant must stop serving what is no longer there.
//
// a single pack whose TOML is broken is NOT an error; it is reported in
// report.skipped, because that pack's commands are gone from the live list.
pub fn reload_list() -> Result<ReloadReport, String> {
    let _gate = SWAP_GATE.lock();

    let fresh = commands::parse_commands_detailed()?;

    let old = crate::commands_list();
    let old_hash = commands::commands_hash(&old);
    let new_hash = commands::commands_hash(&fresh.packs);

    let packs = fresh.packs.len();
    let count = fresh.packs.iter().map(|p| p.commands.len()).sum();
    let skipped = fresh.skipped;

    crate::set_commands_list(fresh.packs);

    Ok(ReloadReport {
        packs,
        commands: count,
        phrases_changed: old_hash != new_hash,
        retrained: false,
        skipped,
        retrain_error: None,
    })
}

#[cfg(feature = "intent")]
static RELOAD_LOCK: Lazy<tokio::sync::Mutex<()>> =
    Lazy::new(|| tokio::sync::Mutex::new(()));

// full reload: publish the new list first, retrain second.
//
// the order matters. everything that is not phrase matching - exe_path,
// exe_args, cli_cmd, cli_args, script, sandbox, timeout, sounds, slots,
// description - is live the instant set_commands_list() returns, with no async
// work at all. that is ~95% of edits. only a changed phrase set pays for the
// classifier.
//
// because of that order a retrain failure can NOT be an Err: the new commands
// are already live by then, and "Err" would tell the caller the opposite. it
// comes back as report.retrain_error - commands live, intent recognition stale.
#[cfg(feature = "intent")]
pub async fn reload_all() -> Result<ReloadReport, String> {
    let _lock = RELOAD_LOCK.lock().await;

    let mut report = reload_list()?;

    if report.phrases_changed {
        match crate::intent::retrain(crate::commands_list()).await {
            Ok(retrained) => report.retrained = retrained,
            Err(e) => {
                error!("Intent retrain failed after a successful command swap: {}", e);
                report.retrain_error = Some(e);
            }
        }
    }

    Ok(report)
}
