use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::fs;
use std::time::Duration;
use std::process::{Child, Command};

use seqdiff::ratio;

mod structs;
pub use structs::*;

#[cfg(windows)]
mod ahk;

use crate::{config, i18n, APP_DIR};

#[cfg(feature = "lua")]
use crate::lua::{self, SandboxLevel, CommandContext};

// what a directory scan actually found. `skipped` names every pack that holds
// a command.toml the parser could not read - those are DROPPED from `packs`,
// and a caller that reports the scan as a clean success without mentioning them
// tells the user their commands are live when they are gone.
#[derive(Debug, Default)]
pub struct ParsedCommands {
    pub packs: Vec<JCommandsList>,
    pub skipped: Vec<String>,
}

// scan resources/commands.
//
// Err ONLY when the directory itself cannot be read - that is the case where we
// know nothing and the caller must keep whatever it already had. A readable but
// empty directory is Ok(empty): after the last pack is deleted the assistant
// must stop serving the commands that are no longer on disk.
pub fn parse_commands_detailed() -> Result<ParsedCommands, String> {
    let mut found = ParsedCommands::default();

    let commands_path = APP_DIR.join(config::COMMANDS_PATH);
    let cmd_dirs = fs::read_dir(&commands_path)
        .map_err(|e| format!("Error reading commands directory {:?}: {}", commands_path, e))?;

    for entry in cmd_dirs.flatten() {
        let cmd_path = entry.path();
        let toml_file = cmd_path.join("command.toml");

        if !toml_file.exists() {
            continue;
        }

        let pack_name = cmd_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();

        let content = match fs::read_to_string(&toml_file) {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to read {}: {}", toml_file.display(), e);
                found.skipped.push(pack_name);
                continue;
            }
        };

        let file: JCommandsList = match toml::from_str(&content) {
            Ok(f) => f,
            Err(e) => {
                warn!("Failed to parse {}: {}", toml_file.display(), e);
                found.skipped.push(pack_name);
                continue;
            }
        };

        found.packs.push(JCommandsList {
            path: cmd_path,
            commands: file.commands,
        });
    }

    found.skipped.sort();

    Ok(found)
}

pub fn parse_commands() -> Result<Vec<JCommandsList>, String> {
    let found = parse_commands_detailed()?;

    if found.packs.is_empty() {
        Err("No commands found".into())
    } else {
        info!("Loaded {} command pack(s)", found.packs.len());
        Ok(found.packs)
    }
}


// change detector for the intent backends: two lists with the same hash train
// the same classifier.
//
// every variable-length part is LENGTH-PREFIXED. concatenating raw bytes made
// ["hello", "world"] and ["helloworld"] hash identically, so moving a word
// across a phrase boundary was invisible - and phrases_changed is the only gate
// between a phrase edit and a classifier that keeps matching the old phrases.
pub fn commands_hash(commands: &[JCommandsList]) -> String {
    use sha2::{Sha256, Digest};

    let mut hasher = Sha256::new();

    fn feed(hasher: &mut Sha256, value: &str) {
        hasher.update((value.len() as u64).to_le_bytes());
        hasher.update(value.as_bytes());
    }

    let lang = i18n::get_language();
    feed(&mut hasher, &lang);

    // collect all command ids and phrases for current language, sorted
    let mut all_data: Vec<(&str, _)> = commands.iter()
        .flat_map(|ac| ac.commands.iter().map(|c| (c.id.as_str(), c.get_phrases(&lang))))
        .collect();
    all_data.sort_by_key(|(id, _)| *id);

    hasher.update((all_data.len() as u64).to_le_bytes());

    for (id, phrases) in all_data {
        feed(&mut hasher, id);
        hasher.update((phrases.len() as u64).to_le_bytes());
        for phrase in phrases.iter() {
            feed(&mut hasher, phrase);
        }
    }

    format!("{:x}", hasher.finalize())
}


pub fn fetch_command<'a>(
    phrase: &str,
    commands: &'a [JCommandsList],
) -> Option<(&'a PathBuf, &'a JCommand)> {
    let lang = i18n::get_language();

    let phrase = phrase.trim().to_lowercase();
    if phrase.is_empty() {
        return None;
    }

    let phrase_chars: Vec<char> = phrase.chars().collect();
    let phrase_words: Vec<&str> = phrase.split_whitespace().collect();

    let mut result: Option<(&PathBuf, &JCommand)> = None;
    let mut best_score = config::CMD_RATIO_THRESHOLD;

    for cmd_list in commands {
        for cmd in &cmd_list.commands {
            let cmd_phrases = cmd.get_phrases(&lang);
            
            for cmd_phrase in cmd_phrases.iter() {
                let cmd_phrase_lower = cmd_phrase.trim().to_lowercase();
                let cmd_phrase_chars: Vec<char> = cmd_phrase_lower.chars().collect();
                
                // character-level similarity
                let char_ratio = ratio(&phrase_chars, &cmd_phrase_chars);
                
                // word-level similarity
                let cmd_words: Vec<&str> = cmd_phrase_lower.split_whitespace().collect();
                let word_score = word_overlap_score(&phrase_words, &cmd_words);
                
                // combined score
                let score = (char_ratio * 0.6) + (word_score * 0.4);
                
                // early exit on perfect match
                if score >= 99.0 {
                    debug!("Perfect match: '{}' -> '{}'", phrase, cmd_phrase_lower);
                    return Some((&cmd_list.path, cmd));
                }
                
                if score > best_score {
                    best_score = score;
                    result = Some((&cmd_list.path, cmd));
                }
            }
        }
    }

    if let Some((_, cmd)) = result {
        info!("Fuzzy match: '{}' -> cmd '{}' (score: {:.1}%)", phrase, cmd.id, best_score);
    } else {
        debug!("No match for '{}' (best: {:.1}%)", phrase, best_score);
    }
    
    result
}


fn word_overlap_score(input_words: &[&str], cmd_words: &[&str]) -> f64 {
    if input_words.is_empty() || cmd_words.is_empty() {
        return 0.0;
    }

    let mut matched = 0.0;
    
    // pre-compute cmd word chars to avoid repeated allocations
    let cmd_word_chars: Vec<Vec<char>> = cmd_words
        .iter()
        .map(|w| w.chars().collect())
        .collect();
    
    for input_word in input_words {
        let input_chars: Vec<char> = input_word.chars().collect();
        
        let best_word_match = cmd_word_chars
            .iter()
            .map(|cw| ratio(&input_chars, cw))
            .fold(0.0_f64, f64::max);
        
        if best_word_match > 70.0 {
            matched += best_word_match / 100.0;
        }
    }

    let max_words = input_words.len().max(cmd_words.len()) as f64;
    (matched / max_words) * 100.0
}




pub fn execute_exe<S: AsRef<OsStr>>(exe: S, args: &[String]) -> std::io::Result<Child> {
    Command::new(exe).args(args).spawn()
}

// run an .ahk source file through the AutoHotkey interpreter installed on this machine
#[cfg(windows)]
fn execute_ahk_script(script: &Path, cmd_config: &JCommand) -> Result<bool, String> {
    ahk::execute_script(script, &cmd_config.exe_args)
}

#[cfg(not(windows))]
fn execute_ahk_script(_script: &Path, cmd_config: &JCommand) -> Result<bool, String> {
    Err(format!("AHK source scripts require Windows (command '{}')", cmd_config.id))
}

pub fn execute_cli(cmd: &str, args: &[String]) -> std::io::Result<Child> {
    debug!("Spawning: cmd /C {} {:?}", cmd, args);

    if cfg!(target_os = "windows") {
        Command::new("cmd").arg("/C").arg(cmd).args(args).spawn()
    } else {
        Command::new("sh").arg("-c").arg(cmd).args(args).spawn()
    }
}

pub fn execute_command(cmd_path: &PathBuf, cmd_config: &JCommand, phrase: Option<&str>, slots: Option<&HashMap<String, SlotValue>>) -> Result<bool, String> {
    // execute command by the type
    match cmd_config.cmd_type.as_str() {

        // BRUH
        "voice" => Ok(true),
        
        // LUA command
        #[cfg(feature = "lua")]
        "lua" => {
            execute_lua_command(cmd_path, cmd_config, phrase, slots)
        }

        // AutoHotkey command - either a compiled .exe or an .ahk source file
        "ahk" => {
            let declared = Path::new(&cmd_config.exe_path);

            // a bare relative path resolves against the process working directory,
            // not the command pack, so only trust it when it is absolute
            let path = if declared.is_absolute() && declared.exists() {
                declared.to_path_buf()
            } else {
                cmd_path.join(&cmd_config.exe_path)
            };

            let is_source = path.extension()
                .map(|e| e.eq_ignore_ascii_case("ahk"))
                .unwrap_or(false);

            if is_source {
                execute_ahk_script(&path, cmd_config)
            } else {
                execute_exe(&path, &cmd_config.exe_args)
                    .map(|_| true)
                    .map_err(|e| format!("AHK process spawn error: {}", e))
            }
        }
        
        // CLI command type
        // @TODO: Consider security restrictions
        "cli" => {
            execute_cli(&cmd_config.cli_cmd, &cmd_config.cli_args)
                .map(|_| true)
                .map_err(|e| format!("CLI command error: {}", e))
        }
        
        // TERMINATOR command (T1000)
        "terminate" => {
            std::thread::sleep(Duration::from_secs(2));
            std::process::exit(0);
        }
        
        // STOP CHANING
        "stop_chaining" => Ok(false),

        // Hand the microphone to the dialogue.
        //
        // Nothing to run: the whole effect is on the listening loop, which sees
        // the type on the way back and switches mode. Ok(false) because it does
        // NOT chain in the ordinary sense - what follows is not another command.
        "dialogue" => Ok(false),

        // other
        _ => {
            error!("Command type unknown: {}", cmd_config.cmd_type);
            Err(format!("Command type unknown: {}", cmd_config.cmd_type).into())
        }
    }
}

// look up a command by its ID
pub fn get_command_by_id<'a>(
    commands: &'a [JCommandsList],
    id: &str,
) -> Option<(&'a PathBuf, &'a JCommand)> {
    for cmd_list in commands {
        for cmd in &cmd_list.commands {
            if cmd.id == id {
                return Some((&cmd_list.path, cmd));
            }
        }
    }
    None
}

pub fn list_paths(commands: &[JCommandsList]) -> Vec<&Path> {
    commands.iter().map(|x| x.path.as_path()).collect()
}

#[cfg(feature = "lua")]
fn execute_lua_command(
    cmd_path: &PathBuf,
    cmd_config: &JCommand,
    phrase: Option<&str>,
    slots: Option<&HashMap<String, SlotValue>>
) -> Result<bool, String> {
    // get script path

    let script_name = if cmd_config.script.is_empty() {
        "script.lua"
    } else {
        &cmd_config.script
    };
    
    let script_path = cmd_path.join(script_name);
    
    if !script_path.exists() {
        return Err(format!("Lua script not found: {}", script_path.display()));
    }
    
    // parse sandbox level
    let sandbox = SandboxLevel::from_str(&cmd_config.sandbox);

    // create context
    let context = CommandContext {
        phrase: phrase.unwrap_or("").to_string(),
        command_id: cmd_config.id.clone(),
        command_path: cmd_path.clone(),
        language: i18n::get_language(),
        slots: slots.map(|s| s.clone()),
    };
    
    // get timeout
    let timeout = Duration::from_millis(cmd_config.timeout);
    
    info!("Executing Lua command: {} (sandbox: {:?}, timeout: {:?})", 
          cmd_config.id, sandbox, timeout);
    
    // execute
    match lua::execute(&script_path, context, sandbox, timeout) {
        Ok(result) => {
            info!("Lua command {} completed (chain: {})", cmd_config.id, result.chain);
            Ok(result.chain)
        }
        Err(e) => {
            error!("Lua command {} failed: {}", cmd_config.id, e);
            Err(e.to_string())
        }
    }
}