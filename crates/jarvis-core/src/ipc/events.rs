use serde::{Deserialize, Serialize};

// Events sent from jarvis-app to GUI
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum IpcEvent {
    // Wake word detected, starting to listen
    WakeWordDetected,
    
    // Actively listening for command
    Listening,
    
    // Speech recognized
    SpeechRecognized { text: String },
    
    // Command was executed
    CommandExecuted { id: String, success: bool },
    
    // Returned to idle state
    Idle,
    
    // Error occurred
    Error { message: String },
    
    // App started
    Started,
    
    // App is shutting down
    Stopping,
    
    // Pong response
    Pong,

    // request GUI to reveal/focus window
    RevealWindow,

    // answer to IpcAction::ReloadCommands.
    //
    // `request_id` echoes the one the action carried. without it a client with
    // two saves in flight - or a second window, or jarvis-cli - resolves its
    // wait on whichever reload finishes first and reports that one's outcome
    // against the wrong save.
    CommandsReloaded {
        request_id: Option<String>,
        // false only when NOTHING was published; see `retrain_error`
        ok: bool,
        packs: usize,
        commands: usize,
        retrained: bool,
        // packs on disk whose TOML does not parse - dropped from the live list
        skipped: Vec<String>,
        // commands ARE live, the intent classifier could not be rebuilt on them
        retrain_error: Option<String>,
        error: Option<String>,
    },
}

// Actions sent from GUI to jarvis-app
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum IpcAction {
    // Request graceful shutdown
    Stop,
    
    // Reload commands from disk. `request_id` is echoed back on the matching
    // CommandsReloaded; #[serde(default)] keeps a bare
    // {"action":"reload_commands"} - what jarvis-cli and older builds send -
    // deserializing exactly as before.
    ReloadCommands {
        #[serde(default)]
        request_id: Option<String>,
    },
    
    // Ping to check connection
    Ping,
    
    // Mute/unmute listening
    SetMuted { muted: bool },

    // Execute text command
    TextCommand { text: String },
}