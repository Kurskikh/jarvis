use jarvis_core::slots;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

// include core
use jarvis_core::{
    audio, audio_processing, commands, config, db, listener, recorder, reload, stt, intent,
    ipc::{self, IpcAction, IpcEvent},
    i18n, voices, models,
    commands_list, set_commands_list,
    APP_CONFIG_DIR, APP_LOG_DIR, DB,
};

// include log
#[macro_use]
extern crate simple_log;
mod log;

// include app
mod app;

// include tray
// @TODO. macOS currently not supported for tray functionality.
#[cfg(not(target_os = "macos"))]
mod tray;

static SHOULD_STOP: AtomicBool = AtomicBool::new(false);

fn main() -> Result<(), String> {
    // initialize directories
    config::init_dirs()?;

    // initialize logging
    log::init_logging()?;

    // log some base info
    info!("Starting Jarvis v{} ...", config::APP_VERSION.unwrap());
    info!("Config directory is: {}", APP_CONFIG_DIR.get().unwrap().display());
    info!("Log directory is: {}", APP_LOG_DIR.get().unwrap().display());

    // initialize settings
    let settings = db::init();

    // set global DB (for core modules that read settings at init time)
    DB.set(settings.arc().clone())
            .expect("DB already initialized");

    // init voices
    let voice_id = settings.lock().voice.clone();
    let language = settings.lock().language.clone();
    if let Err(e) = voices::init(&voice_id, &language) {
        warn!("Failed to init voices: {}", e);
    }

    // init i18n
    i18n::init(&settings.lock().language);

    // init recorder
    if recorder::init().is_err() {
        app::close(1);
    }

    // init models registry (scans available AI models)
    if let Err(e) = models::init() {
        warn!("Models registry init failed: {}", e);
    }

    // clamp backend settings against the registry BEFORE anything consumes them.
    // this is what keeps a bogus intent_backend out of the app::close(1) path below
    settings.sanitize_backends();

    // init stt engine
    if stt::init().is_err() {
        // @TODO. Allow continuing even without STT, if commands is using keywords or smthng?
        app::close(1); // cannot continue without stt
    }

    // init commands
    info!("Initializing commands.");
    let cmds = match commands::parse_commands() {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to parse commands: {}. Starting with empty command list.", e);
            Vec::new()
        }
    };
    info!("Commands initialized. Count: {}, List: {:?}", cmds.len(), commands::list_paths(&cmds));
    set_commands_list(cmds);

    // init audio
    if audio::init().is_err() {
        // @TODO. Allow continuing even without audio?
        app::close(1); // cannot continue without audio
    }

    // init wake-word engine
    if let Err(e) = listener::init() {
        error!("Wake-word engine init failed: {}", e);
        app::close(1);
    }

    // shared async runtime for intent classification, IPC, etc.
    let rt = Arc::new(
        tokio::runtime::Runtime::new().expect("Failed to create tokio runtime")
    );

    // init intent-recognition engine.
    // intent::init() degrades internally (requested backend -> configured
    // default -> "none"), so this is a log line, not an exit: a settings value
    // must never be able to make the assistant quit with no window and no
    // message. The user can still change the backend from the GUI afterwards.
    let cmds = commands_list();
    rt.block_on(async {
        if let Err(e) = intent::init(&cmds).await {
            error!("Intent recognition unavailable: {}", e);
        }
    });

    // init slots parsing engine
    slots::init().map_err(|e| error!("Slot extraction init failed: {}", e)).ok();

    // init audio processing
    info!("Initializing audio processing...");
    if let Err(e) = audio_processing::init() {
        warn!("Audio processing init failed: {}", e);
    }

    // init IPC
    info!("Initializing IPC...");
    ipc::init();

    // channel for text commands (manually written in the GUI)
    let (text_cmd_tx, text_cmd_rx) = mpsc::channel::<String>();

    ipc::set_action_handler(move |action| {
        match action {
            IpcAction::Stop => {
                info!("Received stop command from GUI");
                SHOULD_STOP.store(true, Ordering::SeqCst);
            }
            IpcAction::ReloadCommands { request_id } => {
                info!("Received reload commands request");

                // this closure runs on a tokio worker, INSIDE runtime context and
                // while the IPC server holds the ACTION_HANDLER read guard. so it
                // must return immediately: rt.block_on() here panics ("Cannot
                // start a runtime from within a runtime"), and any long blocking
                // work stalls this client's handle_client select loop, cutting
                // IpcEvent delivery to the GUI that just asked for the reload.
                tokio::spawn(async move {
                    match reload::reload_all().await {
                        Ok(r) => {
                            info!("Commands reloaded: {} pack(s), {} command(s), retrained: {}",
                                  r.packs, r.commands, r.retrained);

                            // these two are NOT failures of the swap - the new
                            // list is live either way - but they are not a
                            // clean reload either, so they travel with ok:true
                            if !r.skipped.is_empty() {
                                warn!("Reload dropped unparseable pack(s): {}", r.skipped.join(", "));
                            }
                            if let Some(ref e) = r.retrain_error {
                                error!("Commands are live but intent recognition is stale: {}", e);
                            }

                            ipc::send(IpcEvent::CommandsReloaded {
                                request_id,
                                ok: true,
                                packs: r.packs,
                                commands: r.commands,
                                retrained: r.retrained,
                                skipped: r.skipped,
                                retrain_error: r.retrain_error,
                                error: None,
                            });
                        }
                        Err(e) => {
                            // reload_all() only returns Err before the swap, so
                            // this really is "nothing changed"
                            error!("Commands reload failed: {}. Previous commands stay active.", e);
                            ipc::send(IpcEvent::CommandsReloaded {
                                request_id,
                                ok: false,
                                packs: 0,
                                commands: 0,
                                retrained: false,
                                skipped: Vec::new(),
                                retrain_error: None,
                                error: Some(e),
                            });
                        }
                    }
                });
            }
            IpcAction::SetMuted { muted } => {
                info!("Received mute request: {}", muted);
                // TODO: implement mute
            }
            IpcAction::TextCommand { text } => {
                info!("Received text command: {}", text);
                if let Err(e) = text_cmd_tx.send(text) {
                    error!("Failed to send text command to app: {}", e);
                }
            }
            IpcAction::Ping => {
                // handled internally by server
            }
            _ => {}
        }
    });

    // start WebSocket server on the shared runtime
    let ipc_rt = Arc::clone(&rt);
    std::thread::spawn(move || {
        ipc_rt.block_on(ipc::start_server());
    });
    
    // start the app (in the background thread)
    let app_rt = Arc::clone(&rt);
    std::thread::spawn(move || {
        let _ = app::start(text_cmd_rx, &app_rt);
    });

    tray::init_blocking(settings);

    Ok(())
}

pub fn should_stop() -> bool {
    SHOULD_STOP.load(Ordering::SeqCst)
}
