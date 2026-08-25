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
// before anything is touched, and it stays written until it has actually been
// given back - not until the turn ends.
//
// That difference is the whole of the third rule, and it is not theoretical.
// Windows files an application's volume under its executable path and hands
// the same level back at the next start, so a reduction that is not undone is
// not temporary: it is permanent, silent, and outlives the build that caused
// it. An application can close mid-answer, and restarting the assistant
// restarts several of its own executables at once - either way the session
// that was written down is gone, and a record thrown away at that moment is a
// volume nobody will ever put back.

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
    // The same identifier without the process id on the end - the key Windows
    // itself files an application's volume under. An application that restarts
    // comes back with a new `id` and the same `key`, and without it a volume
    // taken from something that has since been restarted can never be found
    // again. See plan_restore.
    //
    // Defaulted, because a record written before this field existed still has
    // to be readable: refusing to parse it would strand exactly the volumes it
    // exists to rescue.
    #[serde(default)]
    key: String,
    was: f32,
    set: f32,
    // when it was taken, unix seconds; 0 in records written before this field
    #[serde(default)]
    when: u64,
}

// SetMasterVolume takes a float and GetMasterVolume gives one back; the two
// are not obliged to be bit-identical, and a slider the user has moved will be
// far outside this anyway.
const SAME: f32 = 0.001;

// How long a volume we could not give back is worth remembering.
//
// A record outlives its turn now. An application that closed while ducked is
// not there to be restored, and forgetting it at that moment is precisely how
// a volume stays down for good - Windows files the level by the executable's
// path and hands it back at every future start.
//
// It cannot be kept forever either: a months-old record put back over a level
// the user has since chosen would be its own kind of rudeness, and still_ours
// only catches sliders that were moved, not ones that happen to sit where we
// left them.
const ORPHAN_MAX_AGE: u64 = 7 * 24 * 60 * 60;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// Is this record still worth trying to put back? A zero timestamp means it was
// written before there was one, and is kept: the volume it describes is down
// either way.
fn worth_keeping(t: &Taken, now: u64) -> bool {
    t.when == 0 || now.saturating_sub(t.when) <= ORPHAN_MAX_AGE
}

// Is this volume still the one we left, or has somebody moved it since?
//
// Split out and tested because getting it backwards is invisible: the feature
// would appear to work while quietly undoing every adjustment made during a
// turn.
fn still_ours(current: f32, taken: &Taken) -> bool {
    (current - taken.set).abs() <= SAME
}

// The folder this executable runs from, without the drive letter, lowercased.
//
// A session identifier spells the executable out as a device path -
// \Device\HarddiskVolume14\jarvis\target\release\jarvis-gui.exe - so the
// drive letter of our own path has no counterpart there and the tail is what
// can be compared.
//
// None when the tail would be too short to mean anything: matching on "\" or
// "\bin" would call half the machine ours.
fn own_dir_tail() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let full = dir.to_string_lossy().to_lowercase();
    let tail = match full.find(':') {
        Some(i) => full[i + 1..].to_string(),
        None => full,
    };
    // at least two named segments, e.g. "\jarvis\release"
    if tail.trim_matches('\\').split('\\').filter(|s| !s.is_empty()).count() < 2 {
        return None;
    }
    Some(tail)
}

// Is this session one of ours?
//
// "Ours" is more than this process. The assistant runs as several executables
// out of one folder - the listener speaks the answers, the window plays voice
// previews - and to anyone listening they are all the assistant. Excluding
// only our own process id left the window being ducked along with the music,
// which is exactly what the feature exists to avoid.
fn is_ours(id: &str, pid: u32, own_pid: u32, own_dir: Option<&str>) -> bool {
    if pid == own_pid {
        return true;
    }
    match own_dir {
        Some(dir) => id.to_lowercase().contains(dir),
        None => false,
    }
}

// Sessions we have no business touching.
//
// Ours, or the answer ducks itself. The Windows system sounds, which are
// notification blips nobody wants to hear at a different volume tomorrow. And
// anything not currently making a sound: an idle session's volume is a setting
// the user chose for next time, not noise competing with the mic.
fn skip(id: &str, pid: u32, own_pid: u32, own_dir: Option<&str>, is_system_sounds: bool, active: bool) -> bool {
    is_ours(id, pid, own_pid, own_dir) || is_system_sounds || !active
}

// The readable part of a session identifier: the executable's file name.
fn short_name(id: &str) -> &str {
    let after_path = id.rsplit('\\').next().unwrap_or(id);
    after_path.split("%b").next().unwrap_or(after_path)
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
    // the identifier without the process id; empty if Windows would not say
    key: String,
    volume: ISimpleAudioVolume,
}

// Every session currently making a sound, except the ones we must not touch.
//
// SAFETY: every call here is a COM call on an interface obtained in this same
// function, on a thread that initialised COM in worker(). Nothing crosses a
// thread boundary.
unsafe fn live_sessions(own_pid: u32) -> Result<Vec<Session>, String> {
    let own_dir = own_dir_tail();
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
    let mut ours = Vec::new();
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

        // the identifier is needed BEFORE the decision now: whether a session
        // is one of ours is answered by the path spelled out inside it
        let Ok(id) = control2.GetSessionInstanceIdentifier() else { continue };
        let Ok(id) = id.to_string() else { continue };

        if skip(&id, pid, own_pid, own_dir.as_deref(), is_system, active) {
            // Named, and only ours. "Everything except the assistant" is the
            // whole promise of this feature, and the log is the only place it
            // can be checked from outside. A build that got it wrong turned
            // the assistant's own window down to a fifth and said nothing -
            // and because Windows remembers an application's volume by its
            // path, the mistake outlived the process that made it.
            if active && !is_system && is_ours(&id, pid, own_pid, own_dir.as_deref()) {
                let level = control2
                    .cast::<ISimpleAudioVolume>()
                    .ok()
                    .and_then(|v| v.GetMasterVolume().ok())
                    .unwrap_or(1.0);
                ours.push((short_name(&id).to_string(), level));
            }
            continue;
        }

        let key = session_key(&control2);
        let Ok(volume) = control2.cast::<ISimpleAudioVolume>() else { continue };
        out.push(Session { id, key, volume });
    }
    if !ours.is_empty() {
        debug!(
            "Left alone because they are ours: {}",
            ours.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", ")
        );
        say_if_we_are_quiet(&ours);
    }
    Ok(out)
}

// One of ours sitting below full volume, said out loud once.
//
// Nothing here lowers our own volume - that is the entire point of is_ours -
// but Windows files an application's volume by its path and hands it back at
// every start, so a level put there by an older build survives the build that
// fixed it, and every restart after that. It is inaudible as a cause and
// obvious as a symptom: the assistant simply sounds quiet, which reads as a
// ducking bug that is not there.
//
// A warning and not a correction: the slider belongs to whoever set it, and
// this cannot tell a leftover from a deliberate choice. Saying which one and
// how far down is enough to fix it in the mixer in seconds.
fn say_if_we_are_quiet(ours: &[(String, f32)]) {
    static SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    let quiet: Vec<String> = ours
        .iter()
        .filter(|(_, level)| *level < 1.0 - SAME)
        .map(|(name, level)| format!("{} at {:.0}%", name, level * 100.0))
        .collect();
    if quiet.is_empty() {
        return;
    }
    if SAID.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    warn!(
        "The assistant's own volume is turned down in the Windows mixer: {}. Nothing ducks it on purpose, and Windows remembers the level per application, so this is most likely left over from an older build - put it back in the volume mixer and it will stay.",
        quiet.join(", ")
    );
}

// The identifier without the process id on the end, or empty if Windows will
// not say. Empty never matches anything in plan_restore, which is the right
// answer: better to leave a volume than to hand it to the wrong session.
unsafe fn session_key(control2: &IAudioSessionControl2) -> String {
    control2
        .GetSessionIdentifier()
        .ok()
        .and_then(|k| k.to_string().ok())
        .unwrap_or_default()
}

unsafe fn do_duck(level: f32) -> Result<Vec<Taken>, String> {
    let own = std::process::id();
    let now = now_secs();
    let mut taken = Vec::new();

    let live = live_sessions(own)?;
    debug!(
        "{} session(s) making a sound and not ours: {}",
        live.len(),
        live.iter().map(|s| short_name(&s.id)).collect::<Vec<_>>().join(", ")
    );
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
        taken.push(Taken { id: s.id, key: s.key, was, set, when: now });
    }
    Ok(taken)
}

// Which record, if any, each session in the mixer should be given back.
//
// Two passes, because a session identifier ends in the process id. An
// application that restarted between the duck and the restore no longer owns
// the session that was written down - and a rebuild of the assistant restarts
// several of its executables at once - so an exact search finds nothing, the
// volume is never put back, and Windows hands that same reduced level to every
// future start of that application. That is how a duck lasting one sentence
// became a window permanently at a fifth of its volume.
//
// Exact matches are claimed first, so a second window of an application cannot
// take the record belonging to the first; only what is left over falls back to
// the identifier without the process id.
//
// Pure, and tested, because it decides whether a volume ever comes back and
// there is no way to see it from outside.
fn plan_restore(sessions: &[(String, String)], taken: &[Taken]) -> Vec<(usize, usize)> {
    let mut plan = Vec::new();
    let mut session_claimed = vec![false; sessions.len()];
    let mut record_used = vec![false; taken.len()];

    for (si, (id, _)) in sessions.iter().enumerate() {
        for (ti, t) in taken.iter().enumerate() {
            if !record_used[ti] && t.id == *id {
                plan.push((si, ti));
                session_claimed[si] = true;
                record_used[ti] = true;
                break;
            }
        }
    }

    for (si, (_, key)) in sessions.iter().enumerate() {
        if session_claimed[si] || key.is_empty() {
            continue;
        }
        for (ti, t) in taken.iter().enumerate() {
            if !record_used[ti] && !t.key.is_empty() && t.key == *key {
                plan.push((si, ti));
                session_claimed[si] = true;
                record_used[ti] = true;
                break;
            }
        }
    }

    plan
}

// Give back what the records describe.
//
// Returns which records are settled - put back, or deliberately left alone
// because that slider has been moved since - and how many volumes actually
// changed. A record that is not settled belongs to an application that is not
// running to receive it, and the caller keeps it rather than dropping it:
// dropping it is what left volumes down for good.
unsafe fn do_restore(taken: &[Taken]) -> Result<(Vec<usize>, usize), String> {
    if taken.is_empty() {
        return Ok((Vec::new(), 0));
    }
    // a session that has since stopped is no longer "active", so ask for all
    // of them here rather than only the live ones
    let sessions = live_sessions_any()?;
    let ids: Vec<(String, String)> =
        sessions.iter().map(|s| (s.id.clone(), s.key.clone())).collect();

    let mut settled = Vec::new();
    let mut restored = 0;

    for (si, ti) in plan_restore(&ids, taken) {
        let s = &sessions[si];
        let t = &taken[ti];
        let Ok(current) = s.volume.GetMasterVolume() else { continue };
        if !still_ours(current, t) {
            debug!("Leaving '{}' where you put it ({:.2}, we left {:.2})",
                   short_name(&s.id), current, t.set);
            // that slider belongs to the user now; the record has done its job
            settled.push(ti);
            continue;
        }
        if s.volume.SetMasterVolume(t.was, std::ptr::null()).is_ok() {
            settled.push(ti);
            restored += 1;
        }
    }
    Ok((settled, restored))
}

// Same walk, without the "is it making a sound" filter: by the time a turn
// ends the music may have stopped, and its volume still has to go back.
//
// Nothing is excluded here, our own sessions included. Deciding what may be
// touched is the duck's job; by the time something is written down the
// question is already settled, and restoring can only ever undo something we
// did. Skipping ourselves here meant the one case that needs it most - a
// volume of ours put down by an older build - was the one case that could
// never be repaired.
unsafe fn live_sessions_any() -> Result<Vec<Session>, String> {
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
        let Ok(id) = control2.GetSessionInstanceIdentifier() else { continue };
        let Ok(id) = id.to_string() else { continue };
        let key = session_key(&control2);
        let Ok(volume) = control2.cast::<ISimpleAudioVolume>() else { continue };
        out.push(Session { id, key, volume });
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

    // Everything taken and not yet given back: what a previous run left
    // behind, and later anything a turn could not put back because the
    // application had closed in the meantime.
    let now = now_secs();
    let mut owed: Vec<Taken> = read_state()
        .into_iter()
        .filter(|t| worth_keeping(t, now))
        .collect();
    if !owed.is_empty() {
        info!("Restoring {} volume(s) left ducked by a previous run", owed.len());
        settle(&mut owed);
    }
    persist(&[], &owed);

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
                            debug!("Ducked {} session(s) to {:.0}%", taken.len(), level * 100.0);
                        }
                        held = taken;
                        // on disk BEFORE the turn can go wrong
                        persist(&held, &owed);
                    }
                    Err(e) => warn!("Could not duck: {}", e),
                }
            }
            Cmd::Restore => {
                if held.is_empty() && owed.is_empty() {
                    continue;
                }
                // what this turn took and what older turns still owe are the
                // same job: the application that was missing last time may be
                // running now
                let mut all: Vec<Taken> = held.drain(..).chain(owed.drain(..)).collect();
                settle(&mut all);
                owed = all;
                persist(&[], &owed);
            }
        }
    }

    unsafe { CoUninitialize() };
}

// Put back everything in `list`, and leave in it only what could not be put
// back yet.
fn settle(list: &mut Vec<Taken>) {
    let total = list.len();
    let outcome = unsafe { do_restore(list) };
    match outcome {
        Ok((settled, restored)) => {
            let kept: Vec<Taken> = list
                .drain(..)
                .enumerate()
                .filter(|(i, _)| !settled.contains(i))
                .map(|(_, t)| t)
                .collect();
            *list = kept;
            debug!("Restored {} of {} session(s)", restored, total);
            if !list.is_empty() {
                // Said out loud on purpose. This is the state that used to be
                // thrown away at the end of every turn, and throwing it away
                // is what left an application quiet until somebody found the
                // mixer and worked out why.
                debug!("{} volume(s) still owed - the application is not running; kept for next time",
                       list.len());
            }
        }
        Err(e) => warn!("Could not restore: {}", e),
    }
}

// What is still owed, on disk, so a crash cannot lose it.
fn persist(held: &[Taken], owed: &[Taken]) {
    if held.is_empty() && owed.is_empty() {
        clear_state();
        return;
    }
    let all: Vec<Taken> = held.iter().chain(owed.iter()).cloned().collect();
    write_state(&all);
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
    let now = now_secs();
    let leftover: Vec<Taken> = read_state()
        .into_iter()
        .filter(|t| worth_keeping(t, now))
        .collect();
    if leftover.is_empty() {
        return;
    }
    let done = std::thread::Builder::new()
        .name("ducking-exit".into())
        .spawn(move || {
            let mut list = leftover;
            unsafe {
                if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
                    return;
                }
            }
            settle(&mut list);
            unsafe { CoUninitialize() };
            // anything the exit could not reach stays written down, so the
            // next start can try again
            persist(&[], &list);
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
            let out = match live_sessions_any() {
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
        Taken { id: "s".into(), key: "s".into(), was: 1.0, set, when: 0 }
    }

    fn rec(id: &str, key: &str) -> Taken {
        Taken { id: id.into(), key: key.into(), was: 1.0, set: 0.2, when: 0 }
    }

    fn live(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(i, k)| (i.to_string(), k.to_string())).collect()
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

    // a real identifier, shortened; the device path is what Windows gives
    const GUI: &str = r"{0.0.0.00000000}.{d24c05d6}|\Device\HarddiskVolume14\jarvis\target\release\jarvis-gui.exe%b{0000}|1%b69980";
    const MUSIC: &str = r"{0.0.0.00000000}.{d24c05d6}|\Device\HarddiskVolume2\Users\aleks\AppData\Local\Programs\YandexMusic\Music.exe%b{0000}|1%b14896";
    const OURS: Option<&str> = Some(r"\jarvis\target\release");

    #[test]
    fn our_own_session_is_never_touched() {
        // otherwise the answer ducks itself and the feature is self-defeating
        assert!(skip(MUSIC, 42, 42, OURS, false, true));
    }

    #[test]
    fn another_of_our_executables_is_never_touched() {
        // the window is a different process with a different id, and to
        // anyone listening it is still the assistant. Excluding only our own
        // process ducked it along with the music.
        assert!(is_ours(GUI, 69980, 42, OURS));
        assert!(skip(GUI, 69980, 42, OURS, false, true));
    }

    #[test]
    fn a_stranger_from_another_folder_is_not_ours() {
        assert!(!is_ours(MUSIC, 14896, 42, OURS));
    }

    #[test]
    fn without_a_folder_to_compare_only_our_own_process_is_spared() {
        // a path too short to mean anything must not make half the machine
        // ours - better to duck our own window than to duck nothing
        assert!(is_ours(GUI, 42, 42, None));
        assert!(!is_ours(GUI, 69980, 42, None));
    }

    #[test]
    fn the_comparison_ignores_case() {
        assert!(is_ours(&GUI.to_uppercase(), 69980, 42, OURS));
    }

    #[test]
    fn system_sounds_are_left_alone() {
        assert!(skip(MUSIC, 7, 42, OURS, true, true));
    }

    #[test]
    fn a_silent_application_is_left_alone() {
        // an idle session's volume is a setting chosen for next time, not
        // noise competing with the microphone
        assert!(skip(MUSIC, 7, 42, OURS, false, false));
    }

    #[test]
    fn anything_else_playing_is_fair_game() {
        assert!(!skip(MUSIC, 7, 42, OURS, false, true));
    }

    #[test]
    fn the_short_name_is_the_executable() {
        assert_eq!(short_name(GUI), "jarvis-gui.exe");
        assert_eq!(short_name(MUSIC), "Music.exe");
    }

    // ------------------------------------------------------- putting it back

    #[test]
    fn the_session_we_wrote_down_is_the_one_restored() {
        let taken = vec![rec("app|1%b100", "app")];
        assert_eq!(plan_restore(&live(&[("app|1%b100", "app")]), &taken), vec![(0, 0)]);
    }

    #[test]
    fn an_application_that_restarted_is_still_found() {
        // The bug this exists for. A session identifier ends in the process
        // id, so an application that restarts between the duck and the
        // restore is unrecognisable by it - and rebuilding the assistant
        // restarts several of its own executables at once. Matched by exact
        // id alone, nothing was ever put back, and because Windows remembers
        // an application's volume by its path the reduction survived every
        // restart after that.
        let taken = vec![rec("app|1%b100", "app")];
        assert_eq!(plan_restore(&live(&[("app|1%b777", "app")]), &taken), vec![(0, 0)]);
    }

    #[test]
    fn a_second_window_does_not_take_the_first_ones_record() {
        // two sessions of one application, both written down: each gets its
        // own volume back and not the other's
        let taken = vec![rec("app|1%b100", "app"), rec("app|1%b200", "app")];
        let mut plan = plan_restore(&live(&[("app|1%b200", "app"), ("app|1%b100", "app")]), &taken);
        plan.sort();
        assert_eq!(plan, vec![(0, 1), (1, 0)]);
    }

    #[test]
    fn a_restarted_application_does_not_collect_two_records() {
        // it was playing twice and came back once: one record is paid, the
        // other is still owed rather than applied twice to the same session
        let taken = vec![rec("app|1%b100", "app"), rec("app|1%b200", "app")];
        assert_eq!(plan_restore(&live(&[("app|1%b900", "app")]), &taken).len(), 1);
    }

    #[test]
    fn a_stranger_is_matched_by_neither_pass() {
        let taken = vec![rec("app|1%b100", "app")];
        assert!(plan_restore(&live(&[("other|1%b5", "other")]), &taken).is_empty());
    }

    #[test]
    fn a_record_written_before_keys_existed_only_matches_exactly() {
        // an empty key must not match every session with an empty key, or an
        // old record would put its volume on whatever it found first
        let taken = vec![rec("app|1%b100", "")];
        assert!(plan_restore(&live(&[("app|1%b777", "")]), &taken).is_empty());
        assert_eq!(plan_restore(&live(&[("app|1%b100", "")]), &taken), vec![(0, 0)]);
    }

    #[test]
    fn such_a_record_still_parses() {
        // the file on disk outlives the shape of the struct, and refusing to
        // read it would strand the volumes it describes
        let old = r#"[{"id":"app|1%b100","was":1.0,"set":0.2}]"#;
        let parsed: Vec<Taken> = serde_json::from_str(old).unwrap();
        assert_eq!(parsed.len(), 1);
        assert!(parsed[0].key.is_empty());
        assert_eq!(parsed[0].when, 0);
    }

    #[test]
    fn a_record_nothing_ever_claimed_is_eventually_let_go() {
        let now = 30 * 24 * 60 * 60;
        let mut recent = rec("a", "a");
        recent.when = now - 60;
        let mut ancient = rec("b", "b");
        ancient.when = 1;

        assert!(worth_keeping(&recent, now));
        assert!(!worth_keeping(&ancient, now));
        // written before there was a timestamp to write: kept, because the
        // volume it describes is down either way
        assert!(worth_keeping(&rec("c", "c"), now));
    }
}
