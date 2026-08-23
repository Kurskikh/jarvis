// read / validate / write side of the command pack editor.
//
// this deliberately lives in jarvis-gui and not in jarvis-core: the GUI is the
// only writer, and jarvis-app must never grow a code path that can rewrite the
// pack files while the audio loop is running. everything here is plain
// filesystem work against APP_DIR/resources/commands - the SAME directory
// jarvis-app reads, because APP_DIR is the directory of the running exe and
// both binaries ship side by side.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;

use jarvis_core::commands::{JCommand, JCommandsList, SlotDefinition};
use jarvis_core::{config, voices, APP_DIR};

// the exact set execute_command() dispatches on (jarvis-core commands.rs).
// a plain const, NOT derived from that match: the "lua" arm is itself
// #[cfg(feature = "lua")] and jarvis-cli does not enable it.
pub const COMMAND_TYPES: &[&str] = &["voice", "lua", "ahk", "cli", "terminate", "stop_chaining"];

// lua/sandbox.rs SandboxLevel::from_str() maps anything unknown to Standard
// without a word, so the editor has to reject unknown values itself.
pub const SANDBOX_LEVELS: &[&str] = &["minimal", "standard", "full"];

pub const DEFAULT_TIMEOUT_MS: u64 = 10_000;
pub const MIN_TIMEOUT_MS: u64 = 100;
pub const MAX_TIMEOUT_MS: u64 = 600_000;

// outside resources/commands on purpose: parse_commands() loads ANY subdirectory
// holding a command.toml, so a deleted pack renamed in place would still load
const TRASH_PATH: &str = "resources/.trash/commands";

const SOUND_EXTENSIONS: &[&str] = &["mp3", "wav", "ogg"];

// arrays render inline until the rendered line passes this width
const INLINE_ARRAY_WIDTH: usize = 80;

const HEADER: &str = "# Written by the Jarvis command editor.\n\
                      # Hand edits survive until the next save from the GUI.\n";

#[derive(Serialize, Debug, Clone)]
pub struct PackOnDisk {
    // directory name = the pack's identity
    pub name: String,
    // absolute, display form; feeds show_in_folder
    pub path: String,
    pub commands: Vec<JCommand>,
    // Some(msg) when command.toml exists but does not parse. parse_commands()
    // warn!-skips such a pack; if the editor inherited that the pack would be
    // INVISIBLE and a later save could clobber it unseen.
    pub error: Option<String>,
    // hash of command.toml as it was read. the client echoes it back on save,
    // and a mismatch means someone edited the file meanwhile - which is a
    // designed-in workflow here, because the user is sent into this very folder
    // to author the .lua/.ahk bodies the editor does not manage.
    pub revision: String,
    // false when pack_dir() rejects the directory name. such a pack loads fine
    // in the running assistant but no editor operation can touch it, so it is
    // listed as read-only rather than failing every click with "Invalid pack
    // name" under a title that says the pack was not saved.
    pub managed: bool,
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct PackFiles {
    // *.lua
    pub scripts: Vec<String>,
    // *.ahk, *.exe
    pub executables: Vec<String>,
}

// ### PATHS

pub fn commands_root() -> PathBuf {
    APP_DIR.join(config::COMMANDS_PATH)
}

// every editor entry point takes `pack` straight from the webview. no other
// GUI command accepts a path as INPUT (show_in_folder only passes one out), so
// there is no precedent to copy and this is easy to forget. a whitelist is
// strictly stronger than canonicalize-and-contain and needs no regex dep.
// all 11 shipped pack names pass it.
pub fn pack_dir(pack: &str) -> Result<PathBuf, String> {
    if pack.is_empty() || pack.len() > 64 {
        return Err(format!("Invalid pack name '{}': 1 to 64 characters", pack));
    }
    if !pack.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(format!(
            "Invalid pack name '{}': only letters, digits, '_' and '-' are allowed", pack));
    }
    if pack.starts_with('-') {
        return Err(format!("Invalid pack name '{}': must not start with '-'", pack));
    }

    Ok(commands_root().join(pack))
}

// ### READ SIDE
// always scans fresh - NEVER a Lazy. the editor reads back its own writes.

pub fn list_packs() -> Result<Vec<PackOnDisk>, String> {
    let root = commands_root();

    let entries = fs::read_dir(&root)
        .map_err(|e| format!("Error reading commands directory {}: {}", root.display(), e))?;

    let mut packs: Vec<PackOnDisk> = Vec::new();

    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }

        let toml_path = dir.join("command.toml");
        if !toml_path.exists() {
            continue;
        }

        let name = match dir.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        packs.push(load_pack(name, &dir, &toml_path));
    }

    packs.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(packs)
}

pub fn read_pack(pack: &str) -> Result<PackOnDisk, String> {
    let dir = pack_dir(pack)?;

    let toml_path = dir.join("command.toml");
    if !toml_path.exists() {
        return Err(format!("Command pack '{}' not found", pack));
    }

    Ok(load_pack(pack.to_string(), &dir, &toml_path))
}

pub fn read_pack_raw(pack: &str) -> Result<String, String> {
    let dir = pack_dir(pack)?;
    let toml_path = dir.join("command.toml");

    fs::read_to_string(&toml_path)
        .map_err(|e| format!("Failed to read {}: {}", toml_path.display(), e))
}

pub fn list_pack_files(pack: &str) -> Result<PackFiles, String> {
    let dir = pack_dir(pack)?;

    let mut files = PackFiles::default();

    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        // a pack that was just created has nothing in it yet
        Err(_) => return Ok(files),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        match path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()) {
            Some(ref ext) if ext == "lua" => files.scripts.push(name),
            Some(ref ext) if ext == "ahk" || ext == "exe" => files.executables.push(name),
            _ => {}
        }
    }

    files.scripts.sort();
    files.executables.sort();

    Ok(files)
}

// extension-stripped sound names available for (voice, lang), mirroring
// voices::find_sound_file()'s probe order: <voice>/<lang>/ then <voice>/.
// an empty voice_id means the currently selected voice.
pub fn list_sound_names(voice_id: &str, lang: &str) -> Vec<String> {
    let voice = if voice_id.is_empty() {
        voices::get_current_voice()
    } else {
        voices::get_voice(voice_id)
    };

    let voice = match voice {
        Some(v) => v,
        None => return Vec::new(),
    };

    let mut names: HashSet<String> = HashSet::new();

    collect_sound_names(&voice.path.join(lang), &mut names);
    collect_sound_names(&voice.path, &mut names);

    let mut names: Vec<String> = names.into_iter().collect();
    names.sort();

    names
}

fn collect_sound_names(dir: &Path, out: &mut HashSet<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();

        let is_sound = path.extension()
            .and_then(|e| e.to_str())
            .map(|e| SOUND_EXTENSIONS.contains(&e.to_lowercase().as_str()))
            .unwrap_or(false);

        if !is_sound {
            continue;
        }

        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            out.insert(stem.to_string());
        }
    }
}

fn load_pack(name: String, dir: &Path, toml_path: &Path) -> PackOnDisk {
    let raw = fs::read(toml_path);

    let revision = match raw {
        Ok(ref bytes) => revision_of(bytes),
        Err(_) => String::new(),
    };

    let (commands, error) = match raw {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(content) => match toml::from_str::<JCommandsList>(&content) {
                Ok(parsed) => (parsed.commands, None),
                Err(e) => (Vec::new(), Some(e.to_string())),
            },
            Err(e) => (Vec::new(), Some(format!("command.toml is not valid UTF-8: {}", e))),
        },
        Err(e) => (Vec::new(), Some(format!("Failed to read command.toml: {}", e))),
    };

    PackOnDisk {
        managed: pack_dir(&name).is_ok(),
        name,
        path: dir.display().to_string(),
        commands,
        error,
        revision,
    }
}

// ### LOST-UPDATE GUARD
//
// not a checksum for integrity - just a cheap "is this still the file you
// read?" token. DefaultHasher is enough: the only thing it has to survive is an
// accidental concurrent edit, never an adversary.
fn revision_of(bytes: &[u8]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);

    format!("{:016x}", hasher.finish())
}

fn current_revision(toml_path: &Path) -> String {
    match fs::read(toml_path) {
        Ok(bytes) => revision_of(&bytes),
        // no file: the empty revision, which is what load_pack reports too
        Err(_) => String::new(),
    }
}

// None skips the check outright - that is for callers that create the file.
fn check_revision(toml_path: &Path, expected: Option<&str>) -> Result<(), String> {
    let expected = match expected {
        Some(e) => e,
        None => return Ok(()),
    };

    let actual = current_revision(toml_path);
    if actual == expected {
        return Ok(());
    }

    Err(format!(
        "{} changed on disk after it was opened in the editor. \
         Saving now would overwrite those changes. Reopen the pack and redo your edits.",
        toml_path.display()))
}

// ### SERIALIZER
//
// hand-rolled, not toml::to_string. every optional field on JCommand is
// #[serde(default)] with no skip_serializing_if, so the derived serializer
// would emit description = "", exe_path = "", exe_args = [], timeout = 0 on
// EVERY command, and would walk the three HashMaps in nondeterministic order -
// churning all 11 hand-maintained files on every save.
//
// field order is JCommand's declaration order for the scalars, then phrases,
// sounds, slots. language keys and slot names go through a BTreeMap so two
// saves of the same data produce byte-identical output.
//
// every string goes through toml::Value::String(..).to_string() so quoting and
// escaping are the toml crate's problem, not ours.
pub fn serialize_pack(commands: &[JCommand]) -> Result<String, String> {
    let mut out = String::from(HEADER);

    // an empty file would fail to parse with "missing field `commands`", so an
    // empty pack has to say so explicitly
    if commands.is_empty() {
        out.push_str("\ncommands = []\n");
        return Ok(out);
    }

    for cmd in commands {
        out.push_str("\n[[commands]]\n");

        push_scalar(&mut out, "id", &cmd.id);
        push_scalar(&mut out, "type", &cmd.cmd_type);

        if !cmd.description.is_empty() {
            push_scalar(&mut out, "description", &cmd.description);
        }
        if !cmd.exe_path.is_empty() {
            push_scalar(&mut out, "exe_path", &cmd.exe_path);
        }
        if !cmd.exe_args.is_empty() {
            push_array(&mut out, "exe_args", &cmd.exe_args);
        }
        if !cmd.cli_cmd.is_empty() {
            push_scalar(&mut out, "cli_cmd", &cmd.cli_cmd);
        }
        if !cmd.cli_args.is_empty() {
            push_array(&mut out, "cli_args", &cmd.cli_args);
        }
        if !cmd.script.is_empty() {
            push_scalar(&mut out, "script", &cmd.script);
        }
        if !cmd.sandbox.is_empty() {
            push_scalar(&mut out, "sandbox", &cmd.sandbox);
        }
        if cmd.timeout != 0 {
            out.push_str(&format!("timeout = {}\n", cmd.timeout));
        }

        push_lang_table(&mut out, "phrases", &cmd.phrases);
        push_lang_table(&mut out, "sounds", &cmd.sounds);

        // a slot with an empty entity and no context still emits its header,
        // so it survives the round trip instead of silently disappearing
        let slots: BTreeMap<&String, &SlotDefinition> = cmd.slots.iter().collect();
        for (name, slot) in slots {
            out.push_str(&format!("\n[commands.slots.{}]\n", toml_key(name)));
            push_scalar(&mut out, "entity", &slot.entity);
            if !slot.context.is_empty() {
                push_array(&mut out, "context", &slot.context);
            }
        }
    }

    Ok(out)
}

// a language key whose array is EMPTY is written out, not dropped.
// resolve_localized() (jarvis-core commands/structs.rs) distinguishes the two:
// `ru = []` is deliberate silence for Russian, an absent `ru` falls back to the
// English entry. dropping the key turned one into the other on every save.
fn push_lang_table(out: &mut String, table: &str, map: &HashMap<String, Vec<String>>) {
    let sorted: BTreeMap<&String, &Vec<String>> = map.iter().collect();

    if sorted.is_empty() {
        return;
    }

    out.push_str(&format!("\n[commands.{}]\n", table));
    for (lang, values) in sorted {
        push_array(out, &toml_key(lang), values);
    }
}

fn push_scalar(out: &mut String, key: &str, value: &str) {
    out.push_str(&format!("{} = {}\n", key, toml_string(value)));
}

fn push_array(out: &mut String, key: &str, items: &[String]) {
    let rendered: Vec<String> = items.iter().map(|s| toml_string(s)).collect();

    let inline = format!("{} = [{}]", key, rendered.join(", "));
    if inline.chars().count() <= INLINE_ARRAY_WIDTH {
        out.push_str(&inline);
        out.push('\n');
        return;
    }

    out.push_str(&format!("{} = [\n", key));
    for item in &rendered {
        out.push_str(&format!("    {},\n", item));
    }
    out.push_str("]\n");
}

fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_string()).to_string()
}

// a bare TOML key where the name allows it, a quoted one otherwise
fn toml_key(key: &str) -> String {
    let bare = !key.is_empty()
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');

    if bare {
        key.to_string()
    } else {
        toml_string(key)
    }
}

// ### VALIDATION

// the invariants that hold no matter HOW the file was authored: a unique,
// well-formed id, a type execute_command() dispatches on, a sandbox level
// SandboxLevel::from_str() will not silently rewrite, a bounded timeout.
//
// split out of validate_pack() because raw mode has to enforce these too. it
// deliberately does NOT touch the filesystem: raw mode is also the only way to
// repair a broken pack, and refusing the repair because a .lua file is not
// there yet would close the escape hatch.
//
// `others` is every OTHER pack on disk, needed because get_command_by_id()
// resolves ids GLOBALLY: a duplicate makes one of the two commands permanently
// unreachable.
pub fn validate_structure(pack: &str, commands: &[JCommand]) -> Result<(), String> {
    // id -> the other pack that already owns it
    let mut foreign: HashMap<&str, &str> = HashMap::new();
    let others = list_packs().unwrap_or_default();
    for other in &others {
        if other.name == pack || other.error.is_some() {
            continue;
        }
        for cmd in &other.commands {
            foreign.entry(cmd.id.as_str()).or_insert(other.name.as_str());
        }
    }

    let mut seen: HashSet<&str> = HashSet::new();

    for (i, cmd) in commands.iter().enumerate() {
        let id = cmd.id.trim();

        if id.is_empty() {
            return Err(format!("commands[{}].id: must not be empty", i));
        }
        if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err(format!(
                "commands[{}].id '{}': only letters, digits, '_' and '-' are allowed", i, id));
        }
        if !seen.insert(id) {
            return Err(format!(
                "commands[{}].id '{}': duplicated inside pack '{}'", i, id, pack));
        }
        if let Some(owner) = foreign.get(id) {
            return Err(format!(
                "commands[{}].id '{}': already used by pack '{}'", i, id, owner));
        }

        if !COMMAND_TYPES.contains(&cmd.cmd_type.as_str()) {
            return Err(format!(
                "commands[{}].type '{}': unknown command type (expected one of {})",
                i, cmd.cmd_type, COMMAND_TYPES.join(", ")));
        }

        if !cmd.sandbox.is_empty() && !SANDBOX_LEVELS.contains(&cmd.sandbox.as_str()) {
            return Err(format!(
                "commands[{}].sandbox '{}': expected one of {}",
                i, cmd.sandbox, SANDBOX_LEVELS.join(", ")));
        }

        if cmd.timeout > MAX_TIMEOUT_MS {
            return Err(format!(
                "commands[{}].timeout: must not exceed {} ms", i, MAX_TIMEOUT_MS));
        }
    }

    Ok(())
}

// hard errors - the save is refused, on the FIRST one, naming the field.
// structure first, then the per-type checks that need the pack directory.
pub fn validate_pack(pack: &str, commands: &[JCommand]) -> Result<(), String> {
    let dir = pack_dir(pack)?;

    validate_structure(pack, commands)?;

    for (i, cmd) in commands.iter().enumerate() {
        match cmd.cmd_type.as_str() {
            "lua" => {
                // the name the runtime resolves: `script`, or "script.lua"
                let script = if cmd.script.is_empty() { "script.lua" } else { cmd.script.as_str() };
                if !dir.join(script).exists() {
                    return Err(format!(
                        "commands[{}].script '{}': file not found in pack '{}'", i, script, pack));
                }
                // the serde default of 0 makes a Lua command die on its first VM
                // hook with "Script timeout", so a lua command may not carry it
                if cmd.timeout < MIN_TIMEOUT_MS {
                    return Err(format!(
                        "commands[{}].timeout: must be between {} and {} ms for a lua command",
                        i, MIN_TIMEOUT_MS, MAX_TIMEOUT_MS));
                }
            }
            "ahk" => {
                if cmd.exe_path.trim().is_empty() {
                    return Err(format!("commands[{}].exe_path: required for type 'ahk'", i));
                }
                // an absolute path is only a WARNING: execute_command() trusts it
                // when it exists and otherwise re-joins it against the pack
                let declared = Path::new(&cmd.exe_path);
                if !declared.is_absolute() && !dir.join(declared).exists() {
                    return Err(format!(
                        "commands[{}].exe_path '{}': file not found in pack '{}'",
                        i, cmd.exe_path, pack));
                }
            }
            "cli" => {
                if cmd.cli_cmd.trim().is_empty() {
                    return Err(format!("commands[{}].cli_cmd: required for type 'cli'", i));
                }
            }
            _ => {}
        }
    }

    Ok(())
}

// soft warnings for the UI - never blocks, never writes
pub fn validate_pack_warnings(pack: &str, commands: &[JCommand]) -> Vec<String> {
    let mut warnings: Vec<String> = Vec::new();

    let dir = match pack_dir(pack) {
        Ok(d) => d,
        Err(_) => return warnings,
    };

    // the cross-pack id check in validate_structure() can only see packs that
    // parse. say so, instead of letting a duplicate surface months later as a
    // command that has become permanently unreachable.
    let unreadable: Vec<String> = list_packs().unwrap_or_default().into_iter()
        .filter(|p| p.name != pack && p.error.is_some())
        .map(|p| p.name)
        .collect();

    if !unreadable.is_empty() {
        warnings.push(format!(
            "pack(s) {} do not parse, so their command ids could not be checked for duplicates",
            unreadable.join(", ")));
    }

    // one scan per language, not per command
    let mut sound_cache: HashMap<String, Vec<String>> = HashMap::new();

    for (i, cmd) in commands.iter().enumerate() {
        let id = cmd.id.trim();

        if cmd.phrases.values().all(|values| values.is_empty()) {
            warnings.push(format!(
                "commands[{}] '{}': no phrases, this command can only be reached by id", i, id));
        }

        if cmd.description.trim().is_empty() {
            warnings.push(format!("commands[{}] '{}': no description", i, id));
        }

        // {placeholder} <-> slots, both directions
        let mut used: HashSet<String> = HashSet::new();
        for phrase in cmd.phrases.values().flatten() {
            collect_placeholders(phrase, &mut used);
        }
        for name in &used {
            if !cmd.slots.contains_key(name) {
                warnings.push(format!(
                    "commands[{}] '{}': phrase uses {{{}}} but there is no slot named '{}'",
                    i, id, name, name));
            }
        }
        for (name, slot) in &cmd.slots {
            if !used.contains(name) {
                warnings.push(format!(
                    "commands[{}] '{}': slot '{}' is never used by a phrase", i, id, name));
            }
            if slot.entity.trim().is_empty() {
                warnings.push(format!(
                    "commands[{}] '{}': slot '{}' has no entity, GLiNER has nothing to match",
                    i, id, name));
            }
        }

        // sounds resolve against the CURRENTLY SELECTED voice, exactly like
        // playback does, so a miss here is advisory and never blocks a save
        for (lang, names) in &cmd.sounds {
            let available = sound_cache.entry(lang.clone())
                .or_insert_with(|| list_sound_names("", lang));

            for name in names {
                if !available.contains(name) {
                    warnings.push(format!(
                        "commands[{}] '{}': sound '{}' ({}) is not in the selected voice",
                        i, id, name, lang));
                }
            }
        }

        if cmd.cmd_type == "ahk" {
            let declared = Path::new(&cmd.exe_path);
            if declared.is_absolute() && !declared.exists() && !dir.join(declared).exists() {
                warnings.push(format!(
                    "commands[{}] '{}': exe_path '{}' does not exist on this machine",
                    i, id, cmd.exe_path));
            }
        }
    }

    warnings
}

fn collect_placeholders(phrase: &str, out: &mut HashSet<String>) {
    let mut rest = phrase;

    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let name = after[..close].trim();
                if !name.is_empty() {
                    out.insert(name.to_string());
                }
                rest = &after[close + 1..];
            }
            None => break,
        }
    }
}

// ### WRITE SIDE

pub fn write_pack(pack: &str, commands: &[JCommand], revision: Option<&str>) -> Result<(), String> {
    let dir = pack_dir(pack)?;
    if !dir.is_dir() {
        return Err(format!("Command pack '{}' not found", pack));
    }

    check_revision(&dir.join("command.toml"), revision)?;

    validate_pack(pack, commands)?;

    let rendered = serialize_pack(commands)?;

    write_atomic(&dir, &rendered, Some(commands))
}

// the raw escape hatch: writes the user's text verbatim, so it is the only mode
// that preserves comments and hand formatting.
//
// it skips the filesystem half of validate_pack() - repairing a pack whose
// .lua file is not there yet has to stay possible - but NOT the structural
// half. An empty id, a duplicate id or a type execute_command() does not
// dispatch on is broken at runtime however it got written, and letting it
// through here meant the page reported a hard error as an orange warning next
// to a teal "saved" banner while the assistant hot-reloaded the broken pack.
pub fn write_pack_raw(pack: &str, content: &str, revision: Option<&str>) -> Result<(), String> {
    let dir = pack_dir(pack)?;
    if !dir.is_dir() {
        return Err(format!("Command pack '{}' not found", pack));
    }

    check_revision(&dir.join("command.toml"), revision)?;

    // the toml crate's error carries line and column - pass it through verbatim
    let parsed: JCommandsList = toml::from_str(content).map_err(|e| e.to_string())?;

    validate_structure(pack, &parsed.commands)?;

    write_atomic(&dir, content, None)
}

pub fn create_pack(pack: &str) -> Result<(), String> {
    let dir = pack_dir(pack)?;
    if dir.exists() {
        return Err(format!("Command pack '{}' already exists", pack));
    }

    fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create {}: {}", dir.display(), e))?;

    let rendered = serialize_pack(&[])?;

    // a directory with no command.toml is invisible to list_packs() but still
    // makes the `dir.exists()` check above refuse the name forever, so a failed
    // create must not leave one behind
    if let Err(e) = write_atomic(&dir, &rendered, Some(&[])) {
        let _ = fs::remove_dir(&dir);
        return Err(e);
    }

    Ok(())
}

// MOVE, never remove_dir_all: a pack directory holds user-authored .lua/.ahk
// sources the editor explicitly does not manage, and throwing those away is not
// recoverable. nothing auto-cleans .trash, and the confirmation copy says so.
pub fn trash_pack(pack: &str) -> Result<PathBuf, String> {
    let dir = pack_dir(pack)?;
    if !dir.is_dir() {
        return Err(format!("Command pack '{}' not found", pack));
    }

    let trash = APP_DIR.join(TRASH_PATH);
    fs::create_dir_all(&trash)
        .map_err(|e| format!("Failed to create {}: {}", trash.display(), e))?;

    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();

    let mut target = trash.join(format!("{}-{}", pack, stamp));
    let mut attempt = 1;
    while target.exists() {
        target = trash.join(format!("{}-{}-{}", pack, stamp, attempt));
        attempt += 1;
    }

    fs::rename(&dir, &target)
        .map_err(|e| format!("Failed to move pack '{}' to {}: {}", pack, target.display(), e))?;

    Ok(target)
}

// round-trip check, one rolling backup, then a same-directory atomic replace.
//
// a truncated command.toml would make parse_commands() warn! and `continue`,
// silently dropping the ENTIRE pack from the assistant - so the file is never
// opened for truncation at all.
fn write_atomic(dir: &Path, rendered: &str, expected: Option<&[JCommand]>) -> Result<(), String> {
    let toml_path = dir.join("command.toml");

    // all 11 shipped command.toml files are CRLF and the serializer emits LF
    // only - and an HTML textarea normalises its own value to LF, so raw mode
    // converted too. keep whatever the file already used, so the first save of
    // a pack is not a whole-file diff.
    //
    // skipped when the text contains a multi-line string delimiter: a newline
    // INSIDE ''' or """ is data, and rewriting it would change the value rather
    // than the formatting. the serializer never emits one, so this only ever
    // holds back the raw path.
    let multiline = rendered.contains("'''") || rendered.contains("\"\"\"");

    let rendered = match existing_newline(&toml_path) {
        Newline::Crlf if !multiline => to_crlf(rendered),
        _ => rendered.to_string(),
    };

    // parse the text back BEFORE anything is touched. if the writer ever
    // produces something parse_commands() cannot read, the failure is an Err
    // here rather than a pack that silently vanishes from the assistant.
    let parsed: JCommandsList = toml::from_str(&rendered)
        .map_err(|e| format!("internal: generated TOML failed to re-parse: {}", e))?;

    // field by field, not just the count. a count check cannot see the class of
    // bug this is documented to catch - a lost exe_args, a lost slot, a mangled
    // language map all keep commands.len() intact.
    if let Some(expected) = expected {
        if let Some(diff) = first_difference(expected, &parsed.commands) {
            return Err(format!("internal: generated TOML did not round-trip ({})", diff));
        }
    }


    // the atomic write protects against a TORN file; this protects against the
    // user saving structurally valid garbage over a hand-curated pack. one
    // rolling file cannot accumulate, and .bak is not command.toml so
    // parse_commands() ignores it. a copy failure must not abort the save.
    if toml_path.exists() {
        if let Err(e) = fs::copy(&toml_path, dir.join("command.toml.bak")) {
            log::warn!("Failed to back up {}: {}", toml_path.display(), e);
        }
    }

    // NamedTempFile in the SAME directory so persist() is a same-volume rename
    let mut tmp = tempfile::NamedTempFile::new_in(dir)
        .map_err(|e| format!("Failed to create temp file in {}: {}", dir.display(), e))?;

    tmp.write_all(rendered.as_bytes())
        .map_err(|e| format!("Failed to write {}: {}", toml_path.display(), e))?;
    tmp.flush()
        .map_err(|e| format!("Failed to flush {}: {}", toml_path.display(), e))?;
    tmp.as_file().sync_all()
        .map_err(|e| format!("Failed to sync {}: {}", toml_path.display(), e))?;

    // persist(), not persist_noclobber(): the destination normally exists
    tmp.persist(&toml_path)
        .map_err(|e| format!("Failed to replace {}: {}", toml_path.display(), e))?;

    Ok(())
}

enum Newline {
    Lf,
    Crlf,
}

// what the file on disk already uses. a file that is not there yet, or has no
// line break at all, gets LF.
fn existing_newline(toml_path: &Path) -> Newline {
    match fs::read(toml_path) {
        Ok(bytes) => {
            if bytes.windows(2).any(|w| w == b"\r\n") { Newline::Crlf } else { Newline::Lf }
        }
        Err(_) => Newline::Lf,
    }
}

fn to_crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

// the first field that did not survive the write, or None when every command
// came back identical. names the command and the field so an internal
// serializer bug is actionable instead of just "something was lost".
fn first_difference(expected: &[JCommand], actual: &[JCommand]) -> Option<String> {
    if expected.len() != actual.len() {
        return Some(format!("{} command(s) in, {} back", expected.len(), actual.len()));
    }

    for (i, (a, b)) in expected.iter().zip(actual.iter()).enumerate() {
        let field = if a.id != b.id { "id" }
            else if a.cmd_type != b.cmd_type { "type" }
            else if a.description != b.description { "description" }
            else if a.exe_path != b.exe_path { "exe_path" }
            else if a.exe_args != b.exe_args { "exe_args" }
            else if a.cli_cmd != b.cli_cmd { "cli_cmd" }
            else if a.cli_args != b.cli_args { "cli_args" }
            else if a.script != b.script { "script" }
            else if a.sandbox != b.sandbox { "sandbox" }
            else if a.timeout != b.timeout { "timeout" }
            else if a.phrases != b.phrases { "phrases" }
            else if a.sounds != b.sounds { "sounds" }
            else if !slots_match(&a.slots, &b.slots) { "slots" }
            else { continue };

        return Some(format!("commands[{}].{}", i, field));
    }

    None
}

// SlotDefinition has no PartialEq (it is a jarvis-core type and deriving one
// there for a GUI check would be the wrong place to put it)
fn slots_match(a: &HashMap<String, SlotDefinition>, b: &HashMap<String, SlotDefinition>) -> bool {
    a.len() == b.len() && a.iter().all(|(name, slot)| {
        b.get(name)
            .map(|other| other.entity == slot.entity && other.context == slot.context)
            .unwrap_or(false)
    })
}
