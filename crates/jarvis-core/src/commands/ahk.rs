// AutoHotkey interpreter discovery and .ahk source execution (Windows only).
//
// command packs ship .ahk sources, not compiled .exe files, so the runner has to
// locate an AutoHotkey interpreter installed on this machine. discovery walks the
// registry, the .ahk file association, PATH and the well-known install roots, and
// a successful result is cached - this runs on every voice command.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::DB;

// registry views. AutoHotkey v2 installs from a 64-bit binary and writes to the
// 64-bit view, but the v1.1 setup is itself a 32-bit AutoHotkey build, so under
// WOW64 its writes land in SOFTWARE\WOW6432Node\AutoHotkey. read both.
const KEY_WOW64_64KEY: u32 = 0x0100;
const KEY_WOW64_32KEY: u32 = 0x0200;

const NOT_FOUND_MSG: &str = "AutoHotkey not found: no interpreter in the registry \
(HKCU/HKLM\\SOFTWARE\\AutoHotkey), the .ahk file association, PATH, or the standard \
install directories. Install AutoHotkey from https://www.autohotkey.com - it is picked \
up on the next command, no restart needed. An install somewhere unusual can be pointed \
at with the 'ahk_interpreter' setting key (settings database; not exposed in the UI yet).";

// how an .ahk source file gets run
#[derive(Debug, Clone)]
pub enum AhkRunner {
    // AutoHotkeyUX.exe + launcher.ahk - the launcher inspects the script and
    // dispatches to the matching v1/v2 interpreter.
    Launcher { host: PathBuf, launcher: PathBuf },

    // a concrete interpreter binary, used verbatim. no version dispatch, so the
    // script had better match whatever major version this binary is.
    Direct { exe: PathBuf },
}

impl AhkRunner {
    // the binary that actually gets spawned (for logging / error text)
    pub fn host(&self) -> &Path {
        match self {
            AhkRunner::Launcher { host, .. } => host,
            AhkRunner::Direct { exe } => exe,
        }
    }
}

// discovery result cache. a HIT is cached for the process lifetime; a MISS is
// deliberately NOT cached - jarvis autostarts at login and AutoHotkey may well be
// installed afterwards, and a cached miss would break every .ahk command until a
// restart. probing a machine without AutoHotkey is a handful of failing registry
// opens and stat()s, and it only happens on a command that is about to error out.
static CACHE: Lazy<Mutex<Cache>> = Lazy::new(|| Mutex::new(Cache::default()));

#[derive(Default)]
struct Cache {
    // the 'ahk_interpreter' setting the cached runner was resolved for ("" = none),
    // so that changing the override takes effect without a restart
    key: String,
    runner: Option<AhkRunner>,
}

// the runner to use right now
pub fn runner() -> Result<AhkRunner, String> {
    let configured = configured_override();
    let key = configured
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut cache = CACHE.lock();

    if let Some(runner) = &cache.runner {
        if cache.key == key {
            return Ok(runner.clone());
        }
    }

    let found = match &configured {
        Some(path) => from_override(path),
        None => discover(),
    };

    match found {
        Some(runner) => {
            info!("AutoHotkey runner: {:?}", runner);
            cache.key = key;
            cache.runner = Some(runner.clone());
            Ok(runner)
        }
        None => Err(match &configured {
            Some(path) => format!(
                "Configured AutoHotkey interpreter is not usable: {} (setting 'ahk_interpreter')",
                path.display()
            ),
            None => NOT_FOUND_MSG.to_string(),
        }),
    }
}

// run an .ahk source file through the discovered interpreter.
pub fn execute_script(script: &Path, args: &[String]) -> Result<bool, String> {
    if !script.exists() {
        return Err(format!("AHK script not found: {}", script.display()));
    }

    let runner = runner()?;

    warn_once(&runner, script);

    let mut cmd = Command::new(runner.host());

    if let AhkRunner::Launcher { launcher, .. } = &runner {
        // launcher.ahk is itself a script and has to be the host's own argument.
        // the launcher then consumes leading '/' switches, takes the first argument
        // that is NOT a switch as the script to run and forwards the rest to it, so
        // the ordering here is load-bearing.
        //
        // /Launch makes the launcher hand off and exit. without it the launcher
        // blocks in MsgWaitForMultipleObjects until the script exits (its early-exit
        // check needs ProcessGetParent, which AutoHotkey 2.0 does not have), leaving
        // AutoHotkeyUX.exe - a full v2 interpreter, byte-identical to
        // v2\AutoHotkey64.exe - resident for the script's whole lifetime. we drop the
        // Child without waiting on it anyway, so there is nothing to gain by waiting.
        cmd.arg(launcher).arg("/Launch");
    }

    cmd.arg(script).args(args);

    // a relative #Include resolves against the working directory. the launcher
    // passes NULL for lpCurrentDirectory when it spawns the real interpreter,
    // so whatever we set here propagates all the way down.
    if let Some(dir) = script.parent() {
        cmd.current_dir(dir);
    }

    debug!("Spawning AHK: {:?}", cmd);

    cmd.spawn()
        .map(|_| true)
        .map_err(|e| format!(
            "AHK process spawn error: {} (interpreter: {}, script: {})",
            e,
            runner.host().display(),
            script.display()
        ))
}

// ### DISCOVERY

fn discover() -> Option<AhkRunner> {
    from_registry()
        .or_else(from_association)
        .or_else(from_path)
        .or_else(from_well_known_roots)
}

// absolute path to an interpreter or an install directory, set by the user.
// read defensively - DB is a OnceCell and nothing guarantees it is set by the
// time a command runs.
fn configured_override() -> Option<PathBuf> {
    DB.get()
        .map(|db| db.read().ahk_interpreter.clone())
        .filter(|s| !s.trim().is_empty())
        .map(|s| PathBuf::from(s.trim()))
}

fn from_override(path: &Path) -> Option<AhkRunner> {
    if path.is_dir() {
        return probe_install_dir(path);
    }

    if !path.is_file() {
        return None;
    }

    Some(direct_or_launcher(path.to_path_buf()))
}

// InstallDir from the registry, in AutoHotkey's own lookup order (HKCU first -
// a per-user install must win over a machine-wide one, see UX\inc\common.ahk).
// every root/view is probed rather than just the first one that names an existing
// directory: a stale per-user InstallDir must not mask a working machine-wide
// install.
fn from_registry() -> Option<AhkRunner> {
    for root in [windows_registry::CURRENT_USER, windows_registry::LOCAL_MACHINE] {
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            let key = match root.options().read().access(view).open(r"SOFTWARE\AutoHotkey") {
                Ok(key) => key,
                Err(_) => continue,
            };

            let dir = match key.get_string("InstallDir") {
                Ok(dir) => PathBuf::from(expand_env_vars(&dir)),
                Err(_) => continue,
            };

            if !dir.is_dir() {
                continue;
            }

            // NOTE: this is the installer/UX version, NOT the version of the
            // interpreter a script ends up running under. log only.
            debug!("AutoHotkey InstallDir: {} (installer version: {})",
                   dir.display(),
                   key.get_string("Version").unwrap_or_else(|_| "unknown".into()));

            if let Some(runner) = probe_install_dir(&dir) {
                return Some(runner);
            }
        }
    }

    None
}

// known interpreter layouts under an install directory, first hit wins.
fn probe_install_dir(dir: &Path) -> Option<AhkRunner> {
    // 1. the UX launcher - dispatches v1/v2 per script, so it is the best option
    let host = dir.join("UX").join("AutoHotkeyUX.exe");
    let launcher = dir.join("UX").join("launcher.ahk");
    if host.is_file() && launcher.is_file() {
        return Some(AhkRunner::Launcher { host, launcher });
    }

    // without the launcher there is no per-script version dispatch, and every .ahk
    // that ships with jarvis is v1 syntax (#NoEnv, comma-form commands, %var%), so
    // v1.1 is probed before v2. warn_once() reports the mismatch if a script asks
    // for something this interpreter cannot be.

    // 2. versioned v1.1 subdirectories, newest first
    let mut versions: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && file_name_starts_with(p, "v1.1."))
        .collect();

    versions.sort_by_key(|p| std::cmp::Reverse(version_key(p)));

    for version in versions {
        for name in V1_EXE_NAMES.iter() {
            let candidate = version.join(name);
            if candidate.is_file() {
                return Some(AhkRunner::Direct { exe: candidate });
            }
        }
    }

    // 3. classic root layout (a v1.1 install; AutoHotkey.exe is whatever the
    //    installer copied there)
    // 4. v2 subdirectory
    let mut candidates: Vec<PathBuf> = vec![dir.join("AutoHotkey.exe")];
    candidates.extend(V1_EXE_NAMES.iter().map(|n| dir.join(n)));
    candidates.extend(V2_EXE_NAMES.iter().map(|n| dir.join("v2").join(n)));
    candidates.push(dir.join("v2").join("AutoHotkey.exe"));

    candidates.into_iter()
        .find(|p| p.is_file())
        .map(|exe| AhkRunner::Direct { exe })
}

// the .ahk file association. reached on hand-registered / portable installs that
// wrote no InstallDir.
fn from_association() -> Option<AhkRunner> {
    let classes = windows_registry::CLASSES_ROOT;

    // an explicit user choice wins over the machine-wide association, but it is
    // only a candidate: a UserChoice pointing at something unusable must fall back
    // to HKCR\.ahk rather than abort the whole branch.
    let user_choice = windows_registry::CURRENT_USER
        .options()
        .read()
        .access(KEY_WOW64_64KEY)
        .open(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.ahk\UserChoice")
        .and_then(|key| key.get_string("ProgId"))
        .ok();

    let machine_wide = classes
        .options()
        .read()
        .access(KEY_WOW64_64KEY)
        .open(".ahk")
        .and_then(|key| key.get_string(""))
        .ok();

    [user_choice, machine_wide]
        .into_iter()
        .flatten()
        .find_map(|progid| runner_from_progid(progid.trim()))
}

fn runner_from_progid(progid: &str) -> Option<AhkRunner> {
    if progid.is_empty() {
        return None;
    }

    let command = windows_registry::CLASSES_ROOT
        .options()
        .read()
        .access(KEY_WOW64_64KEY)
        .open(format!(r"{}\Shell\Open\Command", progid))
        .and_then(|key| key.get_string(""))
        .ok()?;

    let tokens = tokenize_command(&command);
    let (host, consumed) = command_executable(&tokens)?;

    // .ahk is very often associated with an EDITOR (VS Code, Notepad++,
    // SciTE4AutoHotkey). handing a script to one of those would open it for
    // editing while execute_script still reports success, so only accept a
    // binary that is named like an AutoHotkey interpreter.
    if !looks_like_interpreter(&host) {
        debug!("Ignoring .ahk association '{}': {} is not an AutoHotkey interpreter",
               progid, host.display());
        return None;
    }

    // everything past the interpreter and its launcher script is placeholders
    // (%1, %*, %L) or switches meant for the shell - never forward them.
    if let Some(second) = tokens.get(consumed) {
        let launcher = PathBuf::from(second);
        if has_ahk_extension(&launcher) && launcher.is_file() {
            return Some(AhkRunner::Launcher { host, launcher });
        }
    }

    Some(direct_or_launcher(host))
}

// an interpreter on PATH. resolved to a concrete path rather than left to
// Command::new's implicit search, so the log line and errors can name it.
fn from_path() -> Option<AhkRunner> {
    let path = std::env::var_os("PATH")?;

    // v1-only names first, for the same reason as probe_install_dir
    let mut names: Vec<&str> = V1_EXE_NAMES.to_vec();
    names.push("AutoHotkey.exe");
    names.extend(V2_EXE_NAMES.iter());

    for name in names {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(AhkRunner::Direct { exe: candidate });
            }
        }
    }

    None
}

fn from_well_known_roots() -> Option<AhkRunner> {
    let roots = [
        std::env::var_os("ProgramFiles").map(|p| PathBuf::from(p).join("AutoHotkey")),
        std::env::var_os("ProgramFiles(x86)").map(|p| PathBuf::from(p).join("AutoHotkey")),
        std::env::var_os("LOCALAPPDATA")
            .map(|p| PathBuf::from(p).join("Programs").join("AutoHotkey")),
    ];

    roots.into_iter()
        .flatten()
        .filter(|dir| dir.is_dir())
        .find_map(|dir| probe_install_dir(&dir))
}

// ### HELPERS

// v1.1 never shipped a plain "AutoHotkey.exe" in its versioned directory, and v2
// never uses the U/A prefixes, so these names identify a major version on sight.
static V1_EXE_NAMES: [&str; 3] = ["AutoHotkeyU64.exe", "AutoHotkeyU32.exe", "AutoHotkeyA32.exe"];
static V2_EXE_NAMES: [&str; 2] = ["AutoHotkey64.exe", "AutoHotkey32.exe"];

// AutoHotkeyUX.exe is byte-identical to v2\AutoHotkey64.exe - it is a plain v2
// interpreter that happens to host launcher.ahk, not a special launcher host. run
// a v1 script through it directly and AutoHotkey answers with a syntax error
// dialog, so pair it with its launcher whenever one sits next to it.
fn direct_or_launcher(host: PathBuf) -> AhkRunner {
    let is_ux = host.file_name()
        .map(|n| n.eq_ignore_ascii_case("AutoHotkeyUX.exe"))
        .unwrap_or(false);

    if is_ux {
        if let Some(launcher) = host.parent().map(|dir| dir.join("launcher.ahk")) {
            if launcher.is_file() {
                return AhkRunner::Launcher { host, launcher };
            }
        }
    }

    AhkRunner::Direct { exe: host }
}

// every interpreter AutoHotkey ships is named AutoHotkey*.exe
fn looks_like_interpreter(path: &Path) -> bool {
    let named = path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.to_ascii_lowercase().starts_with("autohotkey"))
        .unwrap_or(false);

    named && path.extension().map(|e| e.eq_ignore_ascii_case("exe")).unwrap_or(false)
}

// split a shell command string into tokens, respecting double quotes. %VAR% is
// expanded first: a Shell\Open\Command is often REG_EXPAND_SZ and the registry
// hands those back unexpanded.
fn tokenize_command(command: &str) -> Vec<String> {
    let command = expand_env_vars(command);

    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quoted = false;

    for c in command.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    tokens.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            c => {
                current.push(c);
                started = true;
            }
        }
    }

    if started {
        tokens.push(current);
    }

    tokens
}

// the executable a shell command starts with, plus how many tokens it ate. an
// UNQUOTED path with spaces (C:\Program Files\AutoHotkey\AutoHotkey.exe "%1")
// tokenizes into several pieces, so glue them back together until they name a
// real file.
fn command_executable(tokens: &[String]) -> Option<(PathBuf, usize)> {
    let mut joined = tokens.first()?.clone();

    if Path::new(&joined).is_file() {
        return Some((PathBuf::from(joined), 1));
    }

    for (i, token) in tokens.iter().enumerate().skip(1) {
        joined.push(' ');
        joined.push_str(token);

        if Path::new(&joined).is_file() {
            return Some((PathBuf::from(joined), i + 1));
        }
    }

    None
}

// expand %VAR% references, leaving shell placeholders (%1, %*, %L) and unknown
// variables alone.
fn expand_env_vars(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }

    let mut out = String::with_capacity(s.len());
    let mut rest = s;

    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);

        let after = &rest[start + 1..];

        let end = match after.find('%') {
            Some(end) => end,
            None => {
                out.push('%');
                out.push_str(after);
                return out;
            }
        };

        let name = &after[..end];
        let value = if name.is_empty() { None } else { std::env::var_os(name) };

        match value {
            Some(value) => out.push_str(&value.to_string_lossy()),
            None => {
                out.push('%');
                out.push_str(name);
                out.push('%');
            }
        }

        rest = &after[end + 1..];
    }

    out.push_str(rest);
    out
}

fn file_name_starts_with(path: &Path, prefix: &str) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with(prefix))
        .unwrap_or(false)
}

// "v1.1.37.02" -> [1, 1, 37, 2], so that v1.1.37.02 sorts above v1.1.9
fn version_key(path: &Path) -> Vec<u32> {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .trim_start_matches(['v', 'V'])
        .split('.')
        .map(|part| part.parse::<u32>().unwrap_or(0))
        .collect()
}

fn has_ahk_extension(path: &Path) -> bool {
    path.extension()
        .map(|e| e.eq_ignore_ascii_case("ahk"))
        .unwrap_or(false)
}

// ### VERSION DIAGNOSTICS

// scripts already reported by warn_once(). none of the shipped scripts declare
// #Requires, so without this the messages would fire on every voice command.
static WARNED: Lazy<Mutex<HashSet<PathBuf>>> = Lazy::new(|| Mutex::new(HashSet::new()));

// v1 and v2 do not share a syntax, so running a script under the wrong major
// version gets an AutoHotkey syntax-error dialog and nothing in the log. say
// something once per script whenever that outcome is possible. warn! is reserved
// for the cases nothing else is handling; the launcher gets debug!, since it does
// resolve the version itself and is right about it essentially always.
fn warn_once(runner: &AhkRunner, script: &Path) {
    // insert first - on an unreadable script there is nothing useful to say and
    // no point re-reading it on every invocation
    if !WARNED.lock().insert(script.to_path_buf()) {
        return;
    }

    let head = read_head(script);
    let requires = head.as_deref().and_then(requires_major);

    match runner {
        // the launcher identifies a script's version by syntax when there is no
        // #Requires directive, and when that is inconclusive it pops a modal
        // version picker and waits for a human - which looks like a silent hang
        // from here, so leave a breadcrumb for whoever investigates one.
        AhkRunner::Launcher { .. } => {
            if head.is_some() && requires.is_none() {
                debug!("{}: no #Requires directive; the AutoHotkey launcher identifies the \
                        version by syntax and may prompt for it if that is inconclusive",
                       script.display());
            }
        }

        // no launcher means no per-script dispatch: this one interpreter runs
        // everything, right version or not.
        AhkRunner::Direct { exe } => match (requires, exe_major(exe)) {
            (Some(want), Some(have)) if want != have => {
                warn!("{}: requires AutoHotkey v{}, but the only interpreter found is v{} \
                       ({}) - the script will not run",
                      script.display(), want, have, exe.display());
            }
            (None, _) if head.is_some() => {
                warn!("{}: no #Requires directive and no AutoHotkey launcher installed, so \
                       the version cannot be checked - running it under {}",
                      script.display(), exe.display());
            }
            _ => {}
        },
    }
}

fn read_head(script: &Path) -> Option<String> {
    use std::io::Read;

    let mut buf = [0u8; 4096];

    let read = std::fs::File::open(script)
        .and_then(|mut f| f.read(&mut buf))
        .ok()?;

    Some(String::from_utf8_lossy(&buf[..read]).into_owned())
}

// the major version a script asks for, when it says so. syntax-based guessing is
// deliberately left to the launcher.
fn requires_major(head: &str) -> Option<u32> {
    let head = head.to_lowercase();

    let line = head.lines().find(|l| l.trim_start().starts_with("#requires"))?;

    // "#Requires AutoHotkey v2.0", "#Requires AutoHotkey >=1.1.34", and also
    // "#Requires AutoHotkey 64-bit" - which names no version at all
    let major: u32 = line
        .split_once("autohotkey")?
        .1
        .trim_start()
        .trim_start_matches(['<', '>', '=', 'v'])
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()?;

    (major == 1 || major == 2).then_some(major)
}

// the major version of a concrete interpreter, as far as its path gives it away.
// a bare "AutoHotkey.exe" outside a versioned directory is not decidable.
fn exe_major(exe: &Path) -> Option<u32> {
    let name = exe.file_name()?.to_str()?.to_ascii_lowercase();

    // check UX first - it is a v2 binary whose name starts with the v1 prefix
    if name.starts_with("autohotkeyux") {
        return Some(2);
    }
    if name.starts_with("autohotkeyu") || name.starts_with("autohotkeya") {
        return Some(1);
    }
    if V2_EXE_NAMES.iter().any(|n| n.to_ascii_lowercase() == name) {
        return Some(2);
    }

    let parent = exe.parent()?.file_name()?.to_str()?.to_ascii_lowercase();

    if parent.starts_with("v1.") {
        return Some(1);
    }
    if parent == "v2" || parent.starts_with("v2.") {
        return Some(2);
    }

    None
}

// ### TESTS

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_quoted_command() {
        let tokens = tokenize_command(
            r#""C:\Program Files\AutoHotkey\UX\AutoHotkeyUX.exe" "C:\Program Files\AutoHotkey\UX\launcher.ahk" "%1" %*"#
        );

        assert_eq!(tokens[0], r"C:\Program Files\AutoHotkey\UX\AutoHotkeyUX.exe");
        assert_eq!(tokens[1], r"C:\Program Files\AutoHotkey\UX\launcher.ahk");
        assert_eq!(tokens[2], "%1");
        assert_eq!(tokens[3], "%*");
    }

    #[test]
    fn keeps_shell_placeholders_and_unknown_vars() {
        assert_eq!(expand_env_vars(r#"x.exe "%1" %*"#), r#"x.exe "%1" %*"#);
        assert_eq!(expand_env_vars(r"%NoSuchVarHopefully%\a"), r"%NoSuchVarHopefully%\a");
        assert_eq!(expand_env_vars("no percent here"), "no percent here");
    }

    #[test]
    fn expands_known_vars() {
        std::env::set_var("JARVIS_AHK_TEST_VAR", "VALUE");
        assert_eq!(expand_env_vars(r"%JARVIS_AHK_TEST_VAR%\x.exe"), r"VALUE\x.exe");
    }

    #[test]
    fn glues_unquoted_path_with_spaces() {
        // the executable has to exist for the greedy join to terminate, so use
        // one every Windows box has
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let exe = format!(r"{}\System32\notepad.exe", system_root);

        if !Path::new(&exe).is_file() {
            return;
        }

        let tokens = tokenize_command(&format!("{} %1", exe));
        let (host, consumed) = command_executable(&tokens).expect("host");

        assert_eq!(host, Path::new(&exe));
        assert_eq!(tokens.get(consumed).map(String::as_str), Some("%1"));
    }

    #[test]
    fn reads_requires_major() {
        assert_eq!(requires_major("#Requires AutoHotkey v2.0\n"), Some(2));
        assert_eq!(requires_major("; c\n  #Requires AutoHotkey >=1.1.34\n"), Some(1));
        assert_eq!(requires_major("#Requires AutoHotkey 64-bit\n"), None);
        assert_eq!(requires_major("#NoEnv\nSendMode Input\n"), None);
    }

    #[test]
    fn reads_exe_major_from_name_or_directory() {
        assert_eq!(exe_major(Path::new(r"C:\AHK\v1.1.37.02\AutoHotkeyU64.exe")), Some(1));
        assert_eq!(exe_major(Path::new(r"C:\AHK\v1.1.37.02\AutoHotkeyA32.exe")), Some(1));
        assert_eq!(exe_major(Path::new(r"C:\AHK\v2\AutoHotkey64.exe")), Some(2));
        // AutoHotkeyUX.exe is a v2 build despite the v1-looking name prefix
        assert_eq!(exe_major(Path::new(r"C:\AHK\UX\AutoHotkeyUX.exe")), Some(2));
        assert_eq!(exe_major(Path::new(r"C:\AHK\v2\AutoHotkey.exe")), Some(2));
        assert_eq!(exe_major(Path::new(r"C:\AHK\AutoHotkey.exe")), None);
    }

    #[test]
    fn rejects_editors_registered_for_ahk() {
        assert!(looks_like_interpreter(Path::new(r"C:\AHK\AutoHotkeyU64.exe")));
        assert!(looks_like_interpreter(Path::new(r"C:\AHK\UX\AutoHotkeyUX.exe")));
        assert!(!looks_like_interpreter(Path::new(r"C:\VSCode\Code.exe")));
        assert!(!looks_like_interpreter(Path::new(r"C:\SciTE\SciTE.exe")));
        assert!(!looks_like_interpreter(Path::new(r"C:\AHK\AutoHotkey.chm")));
    }
}
