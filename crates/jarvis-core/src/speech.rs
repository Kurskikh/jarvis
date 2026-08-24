// Speaking a language model's answer out loud.
//
// Canned reactions are files on disk, chosen from a voice pack. An answer
// cannot be: nobody knows it in advance, so it has to be synthesised while
// the user waits. That happens in a separate process - CosyVoice needs torch
// and a couple of gigabytes of weights, neither of which belongs in an
// installer - and this module is the client for it.
//
// The shape of the thing is set by two measurements, both in
// docs/superpowers/specs/2026-08-24-llm-voice-streaming-design.md:
//
//   - a synthesis call costs about 1550 ms before it produces anything, plus
//     34 ms per character. That fixed cost is per call, which is why the whole
//     answer goes over in ONE request and there is no sentence chunker here.
//     Cutting the text up would pay the 1550 ms again for every piece.
//
//   - the sidecar's own streaming reaches first speech in about 1.9 s and
//     keeps ahead of playback afterwards, so the audio arrives in pieces that
//     have to be stitched. Stitching is the audio layer's job (see
//     audio::play_speech); this module's job is to get the pieces in order and
//     to stop asking for more when the answer is no longer wanted.

pub mod supervisor;

use std::time::{Duration, Instant};

use crate::{config, DB};

// mirrors the sidecar's frame header
const FLAG_FELL_BACK: u32 = 1 << 1;
const HEADER_LEN: usize = 8;

#[derive(Debug, Clone)]
pub struct SpeechConfig {
    pub url: String,
    pub mode: String,
    pub python: String,
    pub script: String,
    pub instruct: String,
}

#[derive(Debug, Default)]
pub struct Spoken {
    pub frames: usize,
    pub first_frame_ms: u64,
    pub total_ms: u64,
    pub fell_back: bool,
    pub cancelled: bool,
}

#[derive(Debug)]
pub enum SpeechError {
    Disabled,
    Connect { url: String, source: String },
    Transport { url: String, source: String },
    Http { status: u16, body: String },
    // the body ended without the zero-length frame that marks a complete
    // answer. Reported rather than accepted: a truncated answer sounds like a
    // whole one, so silently keeping what arrived would mislead.
    Truncated { frames: usize },
    Playback,
}

impl std::fmt::Display for SpeechError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpeechError::Disabled => write!(f, "speaking answers is switched off"),
            SpeechError::Connect { url, source } => write!(
                f, "nothing is listening at {} - start the speech sidecar ({})", url, source),
            SpeechError::Transport { url, source } =>
                write!(f, "speech sidecar at {} failed: {}", url, source),
            SpeechError::Http { status, body } =>
                write!(f, "speech sidecar returned {}: {}", status, body.trim()),
            SpeechError::Truncated { frames } => write!(
                f, "the answer was cut off after {} chunk(s) - the sidecar stopped mid-answer",
                frames),
            SpeechError::Playback => write!(f, "the audio backend cannot play synthesised speech"),
        }
    }
}

impl std::error::Error for SpeechError {}

impl SpeechConfig {
    pub fn from_settings() -> Result<SpeechConfig, SpeechError> {
        let db = DB.get().ok_or(SpeechError::Disabled)?;
        let s = db.read();
        if !s.llm_speak {
            return Err(SpeechError::Disabled);
        }
        let mode = if config::LLM_TTS_MODES.contains(&s.llm_tts_mode.as_str()) {
            s.llm_tts_mode.clone()
        } else {
            // a hand-edited app.db should not silently pick a mode nobody
            // chose; say which one is being used instead
            warn!("Unknown speech mode '{}', using '{}'",
                  s.llm_tts_mode, config::DEFAULT_LLM_TTS_MODE);
            config::DEFAULT_LLM_TTS_MODE.to_string()
        };
        Ok(SpeechConfig {
            url: s.llm_tts_url.clone(),
            mode,
            python: s.llm_tts_python.clone(),
            script: s.llm_tts_script.clone(),
            instruct: s.llm_tts_instruct.clone(),
        })
    }
}

pub fn is_enabled() -> bool {
    DB.get().map(|db| db.read().llm_speak).unwrap_or(false)
}

// Stop whatever is being spoken. Safe to call at any time, including when
// nothing is speaking.
pub fn stop() {
    crate::audio::stop_speech();
}

// Tags CosyVoice 3 understands, from its tokenizer's additional_special_tokens.
// The model is told it may use the first two; the rest are here because a model
// that has seen them elsewhere may reach for them anyway, and a stray
// "[laughter]" printed in the window would look like a bug.
const SPEECH_TAGS: [&str; 17] = [
    "<strong>", "</strong>", "<laughter>", "</laughter>",
    "[breath]", "[quick_breath]", "[laughter]", "[sigh]", "[cough]",
    "[noise]", "[lipsmack]", "[mn]", "[hissing]", "[clucking]", "[accent]",
    "[vocalized-noise]", "<|endofprompt|>",
];

// Rewrite sloppy tags into the exact form CosyVoice knows.
//
// A model told it may use <strong> writes "<strong >" or "< strong>" often
// enough to matter - one of those reached the window verbatim, next to speech
// that had swallowed it. Both ends suffer: the synthesiser only recognises the
// exact token, so a spaced tag is read as text rather than as emphasis, and
// the stripper only removes what it can match.
//
// Fixing it in one place, before the text is used for either purpose, means
// neither has to be tolerant on its own.
pub fn normalize_tags(text: &str) -> String {
    if !text.contains('<') {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '<' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        // scan to the closing bracket; a '<' with no '>' is just a character
        match chars[i + 1..].iter().position(|&c| c == '>') {
            Some(offset) => {
                let inner: String = chars[i + 1..i + 1 + offset].iter().collect();
                let squeezed: String = inner.chars().filter(|c| !c.is_whitespace()).collect();
                let known = matches!(squeezed.to_ascii_lowercase().as_str(),
                                     "strong" | "/strong" | "laughter" | "/laughter");
                if known {
                    out.push('<');
                    out.push_str(&squeezed.to_ascii_lowercase());
                    out.push('>');
                } else {
                    // not ours - leave it exactly as written
                    out.push('<');
                    out.push_str(&inner);
                    out.push('>');
                }
                i += offset + 2;
            }
            None => {
                out.push(chars[i]);
                i += 1;
            }
        }
    }
    out
}

// The answer without the tags that only mean something to the synthesiser.
//
// The text sent to the sidecar keeps them - they are the whole point - but the
// window and the log show prose. Only this known list is removed: stripping
// anything in square brackets would eat legitimate text, and an answer about,
// say, a config file has every right to contain some.
pub fn strip_markup(text: &str) -> String {
    let mut out = normalize_tags(text);
    if !SPEECH_TAGS.iter().any(|tag| out.contains(tag)) {
        return out;                       // the common case, no allocation churn
    }
    for tag in SPEECH_TAGS {
        out = out.replace(tag, " ");
    }
    // removing a tag from mid-sentence leaves a double space, and one before a
    // comma leaves " ,"
    let mut cleaned = String::with_capacity(out.len());
    let mut last_was_space = false;
    for ch in out.chars() {
        if ch == ' ' {
            if !last_was_space {
                cleaned.push(ch);
            }
            last_was_space = true;
        } else {
            if last_was_space && matches!(ch, ',' | '.' | '!' | '?' | ';' | ':' | '…') {
                cleaned.pop();
            }
            cleaned.push(ch);
            last_was_space = false;
        }
    }
    cleaned.trim().to_string()
}

// Reads the sidecar's length-prefixed frames out of a byte stream that
// arrives in arbitrarily sized pieces. HTTP chunk boundaries have nothing to
// do with frame boundaries, so one network read can carry half a header,
// three whole frames, or the middle of a large one.
struct FrameReader {
    buf: Vec<u8>,
    done: bool,
    flags: u32,
}

impl FrameReader {
    fn new() -> Self {
        FrameReader { buf: Vec::with_capacity(64 * 1024), done: false, flags: 0 }
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    // next complete frame, or None if more bytes are needed
    fn next_frame(&mut self) -> Option<Vec<u8>> {
        if self.done || self.buf.len() < HEADER_LEN {
            return None;
        }
        let len = u32::from_le_bytes(self.buf[0..4].try_into().ok()?) as usize;
        let flags = u32::from_le_bytes(self.buf[4..8].try_into().ok()?);
        if len == 0 {
            self.flags |= flags;
            self.done = true;
            return None;
        }
        if self.buf.len() < HEADER_LEN + len {
            return None;
        }
        self.flags |= flags;
        let payload = self.buf[HEADER_LEN..HEADER_LEN + len].to_vec();
        self.buf.drain(..HEADER_LEN + len);
        Some(payload)
    }
}

// Speak `text`, playing each piece as it arrives.
//
// `cancelled` is checked between pieces. It returns true when this answer has
// been superseded or stopped, at which point the request is dropped - which
// cancels it in flight - and nothing further is queued. There is no cancel
// call to the sidecar: synthesis is not interruptible mid-inference, so
// telling it would change nothing except add an endpoint and a race.
pub async fn say<F>(cfg: &SpeechConfig, text: &str, cancelled: F)
    -> Result<Spoken, SpeechError>
where
    F: Fn() -> bool,
{
    let started = Instant::now();
    let client = reqwest::Client::builder()
        // the sidecar is a local process; a redirect can only lead somewhere
        // it has no business sending the assistant's speech
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| SpeechError::Transport {
            url: cfg.url.clone(),
            source: format!("http client init failed: {}", e),
        })?;

    let url = format!("{}/speak", cfg.url.trim_end_matches('/'));
    // STRIPPED, not normalised. Normalising was right when the engine was
    // CosyVoice, which understands <strong> and [breath]. Qwen3-TTS does not:
    // it reads them out as words. Whatever the model writes, the synthesiser
    // gets prose.
    let body = serde_json::json!({
        "text": strip_markup(text),
        "mode": cfg.mode,
        "instruct": cfg.instruct,
    });

    let resp = client.post(&url).json(&body).send().await.map_err(|e| {
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

    let mut resp = resp;
    let mut reader = FrameReader::new();
    let mut out = Spoken::default();

    loop {
        if cancelled() {
            out.cancelled = true;
            debug!("Speech cancelled after {} chunk(s)", out.frames);
            return Ok(out);
        }

        // the first frame waits on the model; later ones only wait on it
        // continuing, so a long gap there means it stopped
        let budget = Duration::from_secs(if out.frames == 0 {
            config::LLM_TTS_FIRST_FRAME_TIMEOUT
        } else {
            config::LLM_TTS_FRAME_TIMEOUT
        });

        let chunk = match tokio::time::timeout(budget, resp.chunk()).await {
            Ok(Ok(Some(bytes))) => Some(bytes),
            Ok(Ok(None)) => None,
            Ok(Err(e)) => return Err(SpeechError::Transport {
                url: cfg.url.clone(), source: e.to_string() }),
            Err(_) => return Err(SpeechError::Transport {
                url: cfg.url.clone(),
                source: format!("no audio for {}s", budget.as_secs()),
            }),
        };

        match chunk {
            Some(bytes) => reader.feed(&bytes),
            None => break,      // connection closed
        }

        while let Some(payload) = reader.next_frame() {
            if out.frames == 0 {
                out.first_frame_ms = started.elapsed().as_millis() as u64;
            }
            out.frames += 1;
            if !crate::audio::play_speech(payload) {
                return Err(SpeechError::Playback);
            }
            if cancelled() {
                out.cancelled = true;
                return Ok(out);
            }
        }
        if reader.done {
            break;
        }
    }

    // drain anything already buffered before judging the ending
    while let Some(payload) = reader.next_frame() {
        if out.frames == 0 {
            out.first_frame_ms = started.elapsed().as_millis() as u64;
        }
        out.frames += 1;
        if !crate::audio::play_speech(payload) {
            return Err(SpeechError::Playback);
        }
    }

    out.total_ms = started.elapsed().as_millis() as u64;
    out.fell_back = reader.flags & FLAG_FELL_BACK != 0;

    if !reader.done {
        return Err(SpeechError::Truncated { frames: out.frames });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(payload: &[u8], flags: u32) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        v.extend_from_slice(&flags.to_le_bytes());
        v.extend_from_slice(payload);
        v
    }

    fn end(flags: u32) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(&flags.to_le_bytes());
        v
    }

    #[test]
    fn markup_is_stripped_for_display() {
        assert_eq!(
            strip_markup("Это <strong>очень</strong> важно, сэр."),
            "Это очень важно, сэр.");
        assert_eq!(
            strip_markup("Секунду. [breath] Я проверю."),
            "Секунду. Я проверю.");
        // a tag right before punctuation must not leave a floating space
        assert_eq!(strip_markup("Готово[breath], сэр."), "Готово, сэр.");
    }

    #[test]
    fn a_tag_written_sloppily_is_still_a_tag() {
        // seen in the wild: the model wrote "<strong >" and it reached the
        // window as literal text next to speech that had lost the emphasis
        assert_eq!(strip_markup("Ваши показатели в норме. <strong >Все системы штатно."),
                   "Ваши показатели в норме. Все системы штатно.");
        assert_eq!(strip_markup("< strong >важно</ strong >"), "важно");
        assert_eq!(normalize_tags("<strong >x</strong  >"), "<strong>x</strong>");
    }

    #[test]
    fn angle_brackets_that_are_not_ours_survive() {
        // an answer about code or maths has every right to these
        let s = "Если a < b, пишите <config> в файл.";
        assert_eq!(strip_markup(s), s);
        assert_eq!(normalize_tags("<Program Files>"), "<Program Files>");
    }

    #[test]
    fn text_without_markup_is_untouched() {
        // including square brackets that are part of the answer, not markup
        let s = "Правь [server] в файле config.toml, сэр.";
        assert_eq!(strip_markup(s), s);
    }

    #[test]
    fn reads_whole_frames() {
        let mut r = FrameReader::new();
        r.feed(&frame(b"abc", 0));
        r.feed(&frame(b"de", 0));
        r.feed(&end(1));
        assert_eq!(r.next_frame(), Some(b"abc".to_vec()));
        assert_eq!(r.next_frame(), Some(b"de".to_vec()));
        assert_eq!(r.next_frame(), None);
        assert!(r.done);
    }

    #[test]
    fn survives_being_fed_one_byte_at_a_time() {
        // http chunk boundaries have nothing to do with frame boundaries; a
        // header can arrive split across two network reads
        let mut all = frame(b"hello", 0);
        all.extend(end(1));
        let mut r = FrameReader::new();
        let mut got = Vec::new();
        for b in all {
            r.feed(&[b]);
            while let Some(p) = r.next_frame() {
                got.push(p);
            }
        }
        assert_eq!(got, vec![b"hello".to_vec()]);
        assert!(r.done);
    }

    #[test]
    fn several_frames_in_one_read() {
        let mut all = frame(b"one", 0);
        all.extend(frame(b"two", 0));
        all.extend(end(1));
        let mut r = FrameReader::new();
        r.feed(&all);
        assert_eq!(r.next_frame(), Some(b"one".to_vec()));
        assert_eq!(r.next_frame(), Some(b"two".to_vec()));
        assert_eq!(r.next_frame(), None);
        assert!(r.done);
    }

    #[test]
    fn a_body_that_stops_early_is_not_complete() {
        // no zero-length frame: the sidecar died mid-answer
        let mut r = FrameReader::new();
        r.feed(&frame(b"only", 0));
        assert_eq!(r.next_frame(), Some(b"only".to_vec()));
        assert_eq!(r.next_frame(), None);
        assert!(!r.done, "a missing end frame must not read as a finished answer");
    }

    // Talks to a real sidecar, so it is not part of the normal run:
    //   cargo test -p jarvis-core --features llm -- --ignored --nocapture
    //
    // Audio is not initialised in a test process, so play_speech refuses and
    // say() returns Playback. That is the point: reaching Playback means the
    // request went out, the sidecar answered, and a whole frame came back
    // through the reader. Everything up to the speaker is covered, and the
    // one thing a test cannot check is the one thing a person has to hear.
    #[tokio::test]
    #[ignore]
    async fn talks_to_a_live_sidecar() {
        let cfg = SpeechConfig {
            url: config::DEFAULT_LLM_TTS_URL.to_string(),
            mode: "stream".to_string(),
            python: String::new(),
            script: String::new(),
            instruct: String::new(),
        };

        match supervisor::health(&cfg).await {
            Ok(h) => println!("sidecar: {} @ {:?} Hz", h.model, h.sample_rate),
            Err(e) => panic!("no sidecar to test against: {}", e),
        }

        let started = std::time::Instant::now();
        let result = say(&cfg, "Проверка связи, сэр.", || false).await;
        println!("say() returned after {} ms: {:?}", started.elapsed().as_millis(), result);

        match result {
            Err(SpeechError::Playback) => {}     // got audio, nowhere to play it
            Err(e) => panic!("the sidecar seam is broken: {}", e),
            Ok(s) => panic!("audio should not be playable in a test process, got {:?}", s),
        }
    }

    #[test]
    fn flags_survive_to_the_end_frame() {
        let mut r = FrameReader::new();
        r.feed(&frame(b"x", FLAG_FELL_BACK));
        r.feed(&end(1));
        r.next_frame();
        r.next_frame();
        assert!(r.flags & FLAG_FELL_BACK != 0);
    }
}

#[cfg(test)]
mod directive_tests {
    // The directive is written across several source lines. Rust's backslash
    // line-continuation eats the newline AND the indent that follows; without
    // it the literal carries the source's own formatting into the prompt.
    // Cheap to get wrong, invisible in review, so assert it.
    // The engine reads anything it does not recognise out loud. Qwen3-TTS says
    // "Бред" for [breath] and "строк" for <strong>, both measured. So the
    // instruction must not invite a tag, and the wire must not carry one -
    // this pins the first half, strip_markup pins the second.
    #[test]
    fn the_directive_asks_for_no_tags() {
        let d = crate::config::LLM_SPEECH_STYLE_DIRECTIVE;
        for bad in ["<strong>", "[breath]", "<laughter>"] {
            assert!(!d.contains(bad),
                    "the directive still offers {}, which this engine speaks aloud", bad);
        }
        assert!(d.contains("brackets"), "it should say brackets are out: {:?}", d);
        assert!(d.contains("punctuate"), "it must still ask for punctuation: {:?}", d);
    }

    #[test]
    fn the_speech_directive_is_one_clean_line() {
        let d = crate::config::LLM_SPEECH_STYLE_DIRECTIVE;
        assert!(!d.contains('\n'), "directive carries a newline: {:?}", d);
        assert!(!d.contains("  "), "directive carries the source indent: {:?}", d);
        assert!(d.contains("punctuate"), "directive lost its point: {:?}", d);
    }
}
