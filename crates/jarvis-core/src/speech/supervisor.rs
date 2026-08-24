// Keeping the speech sidecar alive.
//
// The sidecar is an ordinary local process, and there are three ways it can
// be there: the owner started it, Jarvis started it, or it is not there at
// all. All three are fine. Speech is a bonus and never a dependency, so
// nothing here is allowed to fail the assistant - the worst case is an answer
// that is written but not spoken.
//
// The one rule that is not negotiable: a process Jarvis spawned is a process
// Jarvis kills. Leaving an orphaned sidecar behind means the next run finds
// the port taken by a copy holding the GPU, and the owner has no idea what is
// holding it.

use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::{SpeechConfig, SpeechError};

// Only ever holds a child WE started. A sidecar that was already running when
// Jarvis came up is somebody else's process and is left alone, including at
// shutdown.
static CHILD: Mutex<Option<Child>> = Mutex::new(None);

// how long to wait for a freshly spawned sidecar to answer. It loads a
// language model and a couple of gigabytes of weights first; ten seconds is
// normal, thirty is a slow disk, past that something is wrong.
const STARTUP_GRACE: Duration = Duration::from_secs(90);
const POLL_EVERY: Duration = Duration::from_millis(500);

#[derive(Debug, Clone)]
pub struct Health {
    pub ok: bool,
    pub model: String,
    pub sample_rate: Option<u32>,
    pub reference: String,
}

// Ask the sidecar whether it is there and ready.
pub async fn health(cfg: &SpeechConfig) -> Result<Health, SpeechError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| SpeechError::Transport {
            url: cfg.url.clone(),
            source: format!("http client init failed: {}", e),
        })?;

    let url = format!("{}/health", cfg.url.trim_end_matches('/'));
    let resp = client.get(&url).send().await.map_err(|e| {
        if e.is_connect() {
            SpeechError::Connect { url: cfg.url.clone(), source: e.to_string() }
        } else {
            SpeechError::Transport { url: cfg.url.clone(), source: e.to_string() }
        }
    })?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(SpeechError::Http { status: status.as_u16(), body });
    }

    let v: serde_json::Value = resp.json().await.map_err(|e| SpeechError::Transport {
        url: cfg.url.clone(),
        source: format!("health response is not json: {}", e),
    })?;

    Ok(Health {
        ok: v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false),
        model: v.get("model").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        sample_rate: v.get("sample_rate").and_then(|x| x.as_u64()).map(|x| x as u32),
        reference: v.get("reference").and_then(|x| x.as_str()).unwrap_or("").to_string(),
    })
}

// True when we are the ones running it.
pub fn is_ours() -> bool {
    CHILD.lock().map(|g| g.is_some()).unwrap_or(false)
}

// Make sure a sidecar is answering, starting one if that is configured and
// none is.
//
// Returns Ok only when something is actually answering. Every failure path
// logs and returns an error the caller is expected to treat as "no speech
// this time", not as a reason to stop.
pub async fn ensure_running(cfg: &SpeechConfig) -> Result<Health, SpeechError> {
    match health(cfg).await {
        Ok(h) => {
            debug!("Speech sidecar already up: {} @ {} Hz",
                   h.model, h.sample_rate.unwrap_or(0));
            return Ok(h);
        }
        Err(SpeechError::Connect { .. }) => {}      // nothing there yet; may start one
        Err(e) => return Err(e),                    // there, but unhappy - do not pile on
    }

    if cfg.python.trim().is_empty() || cfg.script.trim().is_empty() {
        return Err(SpeechError::Connect {
            url: cfg.url.clone(),
            source: "no sidecar running and none configured to start \
                     (set llm_tts_python and llm_tts_script)".to_string(),
        });
    }

    // reap a previous child that has already exited, so its slot does not
    // block a fresh start
    if let Ok(mut guard) = CHILD.lock() {
        if let Some(child) = guard.as_mut() {
            if matches!(child.try_wait(), Ok(Some(_))) {
                warn!("Speech sidecar exited on its own; starting a new one");
                *guard = None;
            } else {
                // running but not answering: it is still loading, or wedged.
                // Either way starting a second one would fight over the port.
                return Err(SpeechError::Connect {
                    url: cfg.url.clone(),
                    source: "sidecar process is running but not answering yet".to_string(),
                });
            }
        }
    }

    let script = std::path::Path::new(cfg.script.trim());
    let dir = script.parent().unwrap_or_else(|| std::path::Path::new("."));
    info!("Starting the speech sidecar: {} {}", cfg.python.trim(), cfg.script.trim());

    let mut command = Command::new(cfg.python.trim());
    command
        .arg(script)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // no console window: this runs behind a tray icon, and a stray black
    // window appearing on the first question would be alarming
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let child = command.spawn().map_err(|e| SpeechError::Transport {
        url: cfg.url.clone(),
        source: format!("cannot start '{}' '{}': {}", cfg.python.trim(), cfg.script.trim(), e),
    })?;

    if let Ok(mut guard) = CHILD.lock() {
        *guard = Some(child);
    }

    let deadline = Instant::now() + STARTUP_GRACE;
    loop {
        tokio::time::sleep(POLL_EVERY).await;

        // if it died while we were waiting, say so now rather than after the
        // full grace period - the reason is in the sidecar's own console
        if let Ok(mut guard) = CHILD.lock() {
            if let Some(child) = guard.as_mut() {
                if let Ok(Some(status)) = child.try_wait() {
                    *guard = None;
                    return Err(SpeechError::Transport {
                        url: cfg.url.clone(),
                        source: format!("the sidecar exited during startup ({})", status),
                    });
                }
            }
        }

        match health(cfg).await {
            Ok(h) => {
                info!("Speech sidecar ready: {} @ {} Hz, reference {}",
                      h.model, h.sample_rate.unwrap_or(0), h.reference);
                return Ok(h);
            }
            Err(_) if Instant::now() < deadline => continue,
            Err(e) => {
                warn!("Speech sidecar did not become ready within {}s",
                      STARTUP_GRACE.as_secs());
                return Err(e);
            }
        }
    }
}

// Stop a sidecar we started. Does nothing to one we merely connected to.
//
// Must be called explicitly before the process exits: std::process::exit runs
// no destructors, which is exactly how the tray used to leave the microphone
// held open.
pub fn shutdown() {
    let Ok(mut guard) = CHILD.lock() else { return };
    let Some(mut child) = guard.take() else { return };

    info!("Stopping the speech sidecar we started");
    if let Err(e) = child.kill() {
        warn!("Could not stop the speech sidecar: {}", e);
        return;
    }
    // reap it, so it does not linger as a zombie holding the port
    let _ = child.wait();
}
