// Turning everything else down while the assistant is being spoken to.
//
// Windows gives every playing application its own volume in the mixer, and
// that is the level this works at rather than the device's. Ducking the device
// would duck the answer along with the music, since the assistant plays
// through the same speakers; ducking per session leaves our own voice alone.
//
// Two rules make the difference between a helpful feature and a rude one.
//
// It does not overwrite a choice you made. If you move an application's slider
// while it is ducked, that slider is left exactly where you put it when the
// turn ends - only volumes still sitting where we left them are put back.
//
// And it survives this process dying. Volumes are other applications' state,
// not ours: a crash while ducked would leave the machine quiet until somebody
// worked out why and fixed it by hand. So what was taken is written to disk
// before anything is touched, and the next start puts it back.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use windows::core::Interface;
use windows::Win32::Media::Audio::{
    eMultimedia, eRender, AudioSessionStateActive, IAudioSessionControl2, IAudioSessionManager2,
    IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
};

use crate::APP_CONFIG_DIR;

// A volume we took, and what we left in its place.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct Taken {
    // stable for the life of a session, and distinguishes two sessions of one
    // process - a browser with two tabs playing has two
    id: String,
    was: f32,
    set: f32,
}

// SetMasterVolume takes a float and GetMasterVolume gives one back; the two
// are not obliged to be bit-identical, and a slider the user has moved will be
// far outside this anyway.
const SAME: f32 = 0.001;

// Is this volume still the one we left, or has somebody moved it since?
//
// Split out and tested because getting it backwards is invisible: the feature
// would appear to work while quietly undoing every adjustment made during a
// turn.
fn still_ours(current: f32, taken: &Taken) -> bool {
    (current - taken.set).abs() <= SAME
}

// Sessions we have no business touching.
//
// Our own, or the answer would duck itself. The Windows system sounds, which
// are notification blips nobody wants to hear at a different volume tomorrow.
// And anything not currently making a sound: an idle session's volume is a
// setting the user chose for next time, not noise competing with the mic.
fn skip(pid: u32, own_pid: u32, is_system_sounds: bool, active: bool) -> bool {
    pid == own_pid || is_system_sounds || !active
}

fn state_path() -> Option<PathBuf> {
    match APP_CONFIG_DIR.get() {
        Some(dir) => Some(dir.join("ducking_state.json")),
        None => {
            // Silence here is how the crash-safety record went missing without
            // anybody noticing: the duck reported success, the file was never
            // written, and the only way to find out was to kill the process
            // and see the volumes stay down.
            warn!("No config directory yet - what was ducked cannot be recorded");
            None
        }
    }
}

fn write_state(taken: &[Taken]) {
    let Some(path) = state_path() else { return };
    match serde_json::to_vec_pretty(taken) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&path, bytes) {
                warn!("Cannot record what was ducked ({}): {}", path.display(), e);
            }
        }
        Err(e) => warn!("Cannot serialise what was ducked: {}", e),
    }
}

fn clear_state() {
    let Some(path) = state_path() else { return };
    if path.exists() {
        if let Err(e) = std::fs::remove_file(&path) {
            warn!("Cannot clear the ducking record ({}): {}", path.display(), e);
        }
    }
}

fn read_state() -> Vec<Taken> {
    let Some(path) = state_path() else { return Vec::new() };
    let Ok(bytes) = std::fs::read(&path) else { return Vec::new() };
    serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        warn!("The ducking record is unreadable and will be ignored: {}", e);
        Vec::new()
    })
}

// ------------------------------------------------------------ the mixer

struct Session {
    id: String,
    volume: ISimpleAudioVolume,
}

// Every session currently making a sound, except the ones we must not touch.
//
// SAFETY: every call here is a COM call on an interface obtained in this same
// function, on a thread that initialised COM in worker(). Nothing crosses a
// thread boundary.
unsafe fn live_sessions(own_pid: u32) -> Result<Vec<Session>, String> {
    let enumerator: IMMDeviceEnumerator =
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
    let device = enumerator
        .GetDefaultAudioEndpoint(eRender, eMultimedia)
        .map_err(|e| e.to_string())?;
    let manager: IAudioSessionManager2 =
        device.Activate(CLSCTX_ALL, None).map_err(|e| e.to_string())?;
    let sessions = manager.GetSessionEnumerator().map_err(|e| e.to_string())?;
    let count = sessions.GetCount().map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for i in 0..count {
        let Ok(control) = sessions.GetSession(i) else { continue };
        let Ok(control2) = control.cast::<IAudioSessionControl2>() else { continue };

        let pid = control2.GetProcessId().unwrap_or(0);
        // NOT .is_ok(). This one returns a raw HRESULT rather than a
        // Result, and it answers S_FALSE for "no, an ordinary session" -
        // which is a success code, so is_ok() is true for every session
        // alive. Read literally that says the whole mixer is system sounds,
        // and nothing gets ducked at all. Measured, not reasoned about: the
        // first run left the music sitting at 100%.
        let is_system = control2.IsSystemSoundsSession() == windows::Win32::Foundation::S_OK;
        let active = control.GetState().map(|s| s == AudioSessionStateActive).unwrap_or(false);
        if skip(pid, own_pid, is_system, active) {
            continue;
        }

        let Ok(id) = control2.GetSessionInstanceIdentifier() else { continue };
        let Ok(id) = id.to_string() else { continue };
        let Ok(volume) = control2.cast::<ISimpleAudioVolume>() else { continue };
        out.push(Session { id, volume });
    }
    Ok(out)
}

unsafe fn do_duck(level: f32) -> Result<Vec<Taken>, String> {
    let own = std::process::id();
    let mut taken = Vec::new();

    let live = live_sessions(own)?;
    debug!("{} session(s) making a sound and eligible", live.len());
    for s in live {
        let Ok(was) = s.volume.GetMasterVolume() else { continue };
        let set = was * level;
        // already quieter than we would make it: leave it be, and do not
        // record it, or restoring would put it UP
        if set >= was {
            continue;
        }
        if s.volume.SetMasterVolume(set, std::ptr::null()).is_err() {
            continue;
        }
        taken.push(Taken { id: s.id, was, set });
    }
    Ok(taken)
}

unsafe fn do_restore(taken: &[Taken]) -> Result<usize, String> {
    if taken.is_empty() {
        return Ok(0);
    }
    let by_id: HashMap<&str, &Taken> = taken.iter().map(|t| (t.id.as_str(), t)).collect();
    let own = std::process::id();
    let mut restored = 0;

    // a session that has since stopped is no longer "active", so ask for all
    // of them here rather than only the live ones
    for s in live_sessions_any(own)? {
        let Some(t) = by_id.get(s.id.as_str()) else { continue };
        let Ok(current) = s.volume.GetMasterVolume() else { continue };
        if !still_ours(current, t) {
            debug!("Leaving '{}' where you put it ({:.2}, we left {:.2})", s.id, current, t.set);
            continue;
        }
        if s.volume.SetMasterVolume(t.was, std::ptr::null()).is_ok() {
            restored += 1;
        }
    }
    Ok(restored)
}

// Same walk, without the "is it making a sound" filter: by the time a turn
// ends the music may have stopped, and its volume still has to go back.
unsafe fn live_sessions_any(own_pid: u32) -> Result<Vec<Session>, String> {
    let enumerator: IMMDeviceEnumerator =
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
    let device = enumerator
        .GetDefaultAudioEndpoint(eRender, eMultimedia)
        .map_err(|e| e.to_string())?;
    let manager: IAudioSessionManager2 =
        device.Activate(CLSCTX_ALL, None).map_err(|e| e.to_string())?;
    let sessions = manager.GetSessionEnumerator().map_err(|e| e.to_string())?;
    let count = sessions.GetCount().map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for i in 0..count {
        let Ok(control) = sessions.GetSession(i) else { continue };
        let Ok(control2) = control.cast::<IAudioSessionControl2>() else { continue };
        if control2.GetProcessId().unwrap_or(0) == own_pid {
            continue;
        }
        let Ok(id) = control2.GetSessionInstanceIdentifier() else { continue };
        let Ok(id) = id.to_string() else { continue };
        let Ok(volume) = control2.cast::<ISimpleAudioVolume>() else { continue };
        out.push(Session { id, volume });
    }
    Ok(out)
}

// ------------------------------------------------------------- the worker

enum Cmd {
    Duck(f32),
    Restore,
}

static TX: Lazy<Mutex<Option<Sender<Cmd>>>> = Lazy::new(|| Mutex::new(None));

// One thread, one COM apartment, one command at a time.
//
// Not the caller's thread: duck() is called from the audio loop, where
// recorder::read_microphone is a blocking pull with a driver-side ring of 1.6
// seconds. Enumerating mixer sessions takes tens of milliseconds, which is
// enough to start losing frames - and losing frames right after the wake word
// means losing the command.
fn worker(rx: mpsc::Receiver<Cmd>) {
    unsafe {
        // this thread's apartment, initialised once and owned for its life
        if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
            error!("Ducking is unavailable: COM refused to start on its thread");
            return;
        }
    }

    // anything left ducked by a previous run, before the first turn
    let leftover = read_state();
    if !leftover.is_empty() {
        info!("Restoring {} volume(s) left ducked by a previous run", leftover.len());
        match unsafe { do_restore(&leftover) } {
            Ok(n) => debug!("Put back {} of {}", n, leftover.len()),
            Err(e) => warn!("Could not put them back: {}", e),
        }
        clear_state();
    }

    let mut held: Vec<Taken> = Vec::new();

    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Duck(level) => {
                // already ducked: a second wake word inside one turn must not
                // duck the ducked volume again, or restoring gets us nowhere
                if !held.is_empty() {
                    continue;
                }
                match unsafe { do_duck(level) } {
                    Ok(taken) => {
                        if !taken.is_empty() {
                            // on disk BEFORE the turn can go wrong
                            write_state(&taken);
                            debug!("Ducked {} session(s) to {:.0}%", taken.len(), level * 100.0);
                        }
                        held = taken;
                    }
                    Err(e) => warn!("Could not duck: {}", e),
                }
            }
            Cmd::Restore => {
                if held.is_empty() {
                    continue;
                }
                match unsafe { do_restore(&held) } {
                    Ok(n) => debug!("Restored {} of {} session(s)", n, held.len()),
                    Err(e) => warn!("Could not restore: {}", e),
                }
                clear_state();
                held.clear();
            }
        }
    }

    unsafe { CoUninitialize() };
}

/// Start the ducking thread. Safe to call more than once.
pub fn init() {
    let mut tx = TX.lock();
    if tx.is_some() {
        return;
    }
    let (sender, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("ducking".into())
        .spawn(move || worker(rx))
        .map(|_| *tx = Some(sender))
        .unwrap_or_else(|e| error!("Cannot start the ducking thread: {}", e));
}

fn send(cmd: Cmd) {
    // self-arming: a caller that forgets init() would otherwise get silence
    // and no error, which is the hardest kind of nothing to debug
    init();
    if let Some(tx) = TX.lock().as_ref() {
        let _ = tx.send(cmd);
    }
}

/// Turn everything else down to `level` of its current volume (0.0 - 1.0).
/// Returns immediately; the work happens on the ducking thread.
pub fn duck(level: f32) {
    send(Cmd::Duck(level.clamp(0.0, 1.0)));
}

/// Put back what we took, leaving alone anything moved in the meantime.
pub fn restore() {
    send(Cmd::Restore);
}

/// Put everything back and wait for it, for the paths that are about to exit.
///
/// The tray leaves through std::process::exit, which runs no destructors and
/// gives the worker no chance to see the command - the volumes would stay down
/// and only the next start would fix them. Better to be a few milliseconds
/// slower on the way out.
pub fn restore_blocking() {
    let leftover = read_state();
    if leftover.is_empty() {
        return;
    }
    let done = std::thread::Builder::new()
        .name("ducking-exit".into())
        .spawn(move || {
            unsafe {
                if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
                    return;
                }
                let _ = do_restore(&leftover);
                CoUninitialize();
            }
            clear_state();
        });
    if let Ok(handle) = done {
        let _ = handle.join();
    }
}

/// Every session in the mixer right now, with its volume.
///
/// For looking, never for deciding: the ducking itself reads volumes on its
/// own thread at the moment it acts. This exists so the behaviour can be
/// measured from outside instead of taken on trust.
pub fn snapshot() -> Vec<(String, f32)> {
    let handle = std::thread::Builder::new()
        .name("ducking-snapshot".into())
        .spawn(|| unsafe {
            if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
                return Vec::new();
            }
            let out = match live_sessions_any(0) {
                Ok(sessions) => sessions
                    .into_iter()
                    .map(|s| {
                        let v = s.volume.GetMasterVolume().unwrap_or(-1.0);
                        (s.id, v)
                    })
                    .collect(),
                Err(_) => Vec::new(),
            };
            CoUninitialize();
            out
        });
    handle.and_then(|h| h.join().map_err(|_| std::io::Error::other("panic")))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn taken(set: f32) -> Taken {
        Taken { id: "s".into(), was: 1.0, set }
    }

    #[test]
    fn a_volume_still_where_we_left_it_is_ours_to_restore() {
        assert!(still_ours(0.2, &taken(0.2)));
    }

    #[test]
    fn a_volume_the_user_has_moved_is_left_alone() {
        // the whole point: turning the music up during an answer is a decision,
        // and the end of the turn must not quietly undo it
        assert!(!still_ours(0.8, &taken(0.2)));
        assert!(!still_ours(0.05, &taken(0.2)));
    }

    #[test]
    fn tiny_float_drift_does_not_count_as_moved() {
        // SetMasterVolume and GetMasterVolume are not obliged to agree bit for
        // bit, and treating that as "the user moved it" would restore nothing
        assert!(still_ours(0.2 + 0.0005, &taken(0.2)));
    }

    #[test]
    fn our_own_session_is_never_touched() {
        // otherwise the answer ducks itself and the feature is self-defeating
        assert!(skip(42, 42, false, true));
    }

    #[test]
    fn system_sounds_are_left_alone() {
        assert!(skip(7, 42, true, true));
    }

    #[test]
    fn a_silent_application_is_left_alone() {
        // an idle session's volume is a setting chosen for next time, not
        // noise competing with the microphone
        assert!(skip(7, 42, false, false));
    }

    #[test]
    fn anything_else_playing_is_fair_game() {
        assert!(!skip(7, 42, false, true));
    }
}
