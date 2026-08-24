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

    // an LLM turn started for an utterance no command matched.
    //
    // sent from a task spawned off the audio thread, so it arrives AFTER the
    // Idle of the turn that started it (app.rs sends Idle unconditionally). the
    // GUI must treat it as independent of the jarvisState machine, not as a state.
    LlmThinking {
        request_id: String,
        prompt: String,
    },

    // the answer, or why there is none. one of these follows every LlmThinking
    // with the same request_id, EXCEPT when the turn was superseded: a new
    // utterance retires the one in flight (app.rs supersede_llm_turn) and the
    // dropped turn says nothing. that is not a stranded spinner - the
    // SpeechRecognized of the utterance that superseded it clears the panel
    // first, and the frontend also clears it when the socket closes, on
    // Stopping, and on an explicit disconnect. those four are the complete set
    // of ways a turn can end without an answer.
    //
    // one terminal variant rather than two: the GUI has one panel with one
    // lifecycle, so there is exactly one event that clears the spinner and it
    // cannot get stuck because a case was forgotten.
    //
    // `error_code` is LlmError::code() - a stable discriminant the GUI turns
    // into a localized headline (llm-error-<code>). `error` is the composed
    // English message, the only thing that names the endpoint, the model and
    // the server's own words; shown as detail. same split as CommandsReloaded:
    // localized frame, raw detail.
    LlmAnswer {
        request_id: String,
        prompt: String,
        answer: Option<String>,
        model: String,
        elapsed_ms: u64,
        error_code: Option<String>,
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
    
    // Re-read the LLM settings from app.db.
    //
    // jarvis-app loads app.db once at startup and the settings window lives in
    // a different process, so without this nothing saved there ever reaches the
    // running assistant. fired by the GUI after a successful db_write_many;
    // handled by db::reload_live_settings(), which adopts the settings a
    // running assistant can take up without restarting - the two engines,
    // the wake threshold, the VAD levels, ducking, and the llm_* keys
    // and the openai token - the rest were consumed at init and still need a
    // restart. fire-and-forget: there is no answering event, the next LLM turn
    // simply uses the new values.
    ReloadSettings,

    // Ping to check connection
    Ping,
    
    // Mute/unmute listening
    SetMuted { muted: bool },

    // Execute text command
    TextCommand { text: String },

    // Cut short whatever the assistant is saying.
    //
    // The microphone is deaf while a reaction plays (audio::is_speaking), so
    // a spoken answer cannot be interrupted by voice - there has to be a way
    // that does not go through the microphone at all. Sent by the window and
    // by the tray.
    StopSpeaking,
}