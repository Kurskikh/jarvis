// Read-only probes the settings screen makes before anything is saved.
//
// Both answer the same kind of question - is the thing at this address there,
// and what does it have? - and both are asked with the values TYPED in the
// form rather than the ones in the database, because the whole point is to
// find out before committing to them.
//
// Neither writes anything. The model list is a list of choices; the sidecar
// check reports what the sidecar says about itself. Which model the sidecar
// runs is changed in the sidecar's own console, so that setting keeps exactly
// one owner and the two screens cannot disagree.

use jarvis_core::db::structs::is_loopback_url;
use jarvis_core::llm;
use jarvis_core::speech::{supervisor, SpeechConfig};
use serde::Serialize;

// A human is watching a settings screen while this runs, so it gets a short
// fixed budget rather than llm_timeout - that one is sized for a cold model
// load and would leave the screen looking frozen for a minute.
const PROBE_TIMEOUT_SECS: u64 = 10;

// Ask an OpenAI-compatible server which models it can serve.
//
// allow_remote is passed in rather than read from the database for the same
// reason as the address: the screen is asking about settings that have not
// been saved. jarvis_core::llm::client applies the offline-first gate to the
// pair, so a remote address with the flag off is refused here exactly as it
// would be refused during a real turn.
#[tauri::command]
pub async fn list_llm_models(
    base_url: String,
    api_key: String,
    allow_remote: bool,
) -> Result<Vec<String>, String> {
    llm::list_models(&base_url, &api_key, allow_remote, PROBE_TIMEOUT_SECS)
        .await
        .map_err(|e| e.to_string())
}

// What the settings screen shows next to the sidecar address.
#[derive(Serialize)]
pub struct SidecarStatus {
    pub model: String,
    pub sample_rate: Option<u32>,
    // the voice sample the sidecar cloned from, as the sidecar reports it
    pub reference: String,
}

// Ask the speech sidecar whether it is up and what it loaded.
//
// Loopback only, and with no setting to loosen it. The sidecar is a local
// process by definition - it exists to keep synthesis on this machine - so
// there is no address for it worth reaching that is not on this machine, and
// a probe that would send one anywhere else has nothing to be for.
#[tauri::command]
pub async fn check_speech_sidecar(url: String) -> Result<SidecarStatus, String> {
    let url = url.trim().trim_end_matches('/').to_string();
    if url.is_empty() {
        return Err("the address is empty - there is nothing to check.".to_string());
    }
    if !is_loopback_url(&url) {
        return Err(format!(
            "'{}' is not a loopback address. The speech sidecar is a local process; \
             jarvis will not send your voice anywhere else.",
            url
        ));
    }

    let cfg = SpeechConfig {
        url,
        mode: String::new(),
        python: String::new(),
        script: String::new(),
        instruct: String::new(),
    };

    let health = supervisor::health(&cfg).await.map_err(|e| e.to_string())?;
    // answering and being ready are different things: the sidecar serves
    // /health while it is still loading a model onto the GPU, and reporting
    // that as "connected" would promise a voice that is not there yet.
    if !health.ok {
        return Err(format!(
            "the sidecar answered but reports it is not ready (model: '{}').",
            health.model
        ));
    }
    Ok(SidecarStatus {
        model: health.model,
        sample_rate: health.sample_rate,
        reference: health.reference,
    })
}
