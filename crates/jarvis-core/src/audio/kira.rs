use once_cell::sync::OnceCell;
use std::path::PathBuf;
use std::sync::Mutex;

// use kira::{
//     manager::{backend::DefaultBackend, AudioManager, AudioManagerSettings},
//     sound::static_sound::{StaticSoundData, StaticSoundSettings},
// };

use kira::{
    AudioManager, AudioManagerSettings, DefaultBackend, StartTime, Tween,
    clock::{ClockHandle, ClockSpeed, ClockTime},
    sound::{PlaybackState, static_sound::{StaticSoundData, StaticSoundHandle}},
};

static MANAGER: OnceCell<Mutex<AudioManager>> = OnceCell::new();

// Playing a spoken answer means playing a run of clips that must sound like
// one continuous utterance. They arrive one at a time, each while the previous
// one is still playing, so they cannot simply be played on arrival: two
// `play()` calls overlap, and a chain of `StartTime::Delayed` accumulates
// error until the seams are audible.
//
// Instead a clock runs for the lifetime of the process and every clip is
// pinned to the absolute tick where the one before it ends. At a thousand
// ticks a second the seams land inside a millisecond, and the error does not
// compound because each start time is computed from the running total rather
// than from the previous clip.
static SEQUENCE: Mutex<Option<Sequence>> = Mutex::new(None);

// far enough ahead that scheduling never lands in the past between reading the
// clock and the sound reaching the mixer, short enough not to be heard
const SCHEDULE_LEAD_MS: u64 = 60;

// How long to sit on the first chunk of an utterance before letting it play.
//
// A jitter buffer, and it is not optional. CosyVoice starts a stream with a
// small chunk and grows the next ones, so the first piece of audio is about
// half a second long while the piece after it takes over a second to make.
// Playing the moment the first chunk lands therefore guarantees running dry
// almost immediately - measured at 950 ms of silence on a real answer.
//
// Waiting here costs the same amount at the start, once, where the "thinking"
// clip is still covering. Paying it up front buys an answer that does not
// stumble in the middle, which is the half nobody can mask.
const FIRST_CHUNK_PREROLL_MS: u64 = 900;

// The pre-roll adapts, because a fixed one cannot be right.
//
// 900 ms was measured on this machine and cut the stalls from 950 ms to
// somewhere between 40 and 220 - better, not gone. Raising the constant until
// the worst case fits would make every answer on every machine wait for the
// worst case, which is the wrong trade: the delay is paid on every single
// answer while the stall happens on some of them.
//
// So the buffer remembers. Each time playback runs dry, the shortfall is
// added to the next utterance's head start; when answers stop stalling it
// decays back down. A machine that never stalls keeps the 900 ms; a slower one
// finds its own number within two or three answers instead of needing a
// constant edited by hand.
const PREROLL_MAX_MS: u64 = 2500;
const PREROLL_DECAY_MS: u64 = 100;   // shed per clean utterance
static EXTRA_PREROLL_MS: Mutex<u64> = Mutex::new(0);

struct Sequence {
    clock: ClockHandle,
    // absolute tick where the next clip should start; 0 means nothing queued
    next_tick: u64,
    playing: Vec<StaticSoundHandle>,
}

pub fn init() -> Result<(), ()> {
    if MANAGER.get().is_some() {
        return Ok(());
    }  // already initialized

    // Create an audio manager. This plays sounds and manages resources.
    match AudioManager::<DefaultBackend>::new(AudioManagerSettings::default()) {
        Ok(manager) => {
            // store
            MANAGER.set(Mutex::new(manager)).ok();

            // success
            Ok(())
        }
        Err(msg) => {
            error!("Failed to initialize audio stream.\nError details: {}", msg);

            // failed
            Err(())
        }
    }
}

// @TODO. Cache sounds in memory? With a pool of a certain size, for instance.
// returns how long the clip will play, so the caller can stop listening for
// that long - the microphone hears the speakers
pub fn play_sound(filename: &PathBuf) -> Option<std::time::Duration> {
    // load the file
    match StaticSoundData::from_file(filename) {
        Ok(sound_data) => {
            let duration = sound_data.duration();

            // play it (non-blocking)
            if let Some(manager) = MANAGER.get() {
                if let Ok(mut audio_manager) = manager.lock() {
                    if let Err(e) = audio_manager.play(sound_data) {
                        warn!("Failed to play sound: {}", e);
                        return None;
                    }
                }
            } else {
                warn!("Audio manager not initialized");
                return None;
            }

            Some(duration)
        }
        Err(err) => {
            warn!("Cannot find sound file: {} (err: {})", filename.display(), err);
            None
        }
    }
}

// Queue one piece of an utterance so it starts exactly where the previous
// piece ends. Takes the encoded bytes rather than a path: these arrive over a
// socket and never touch disk.
//
// Returns how long from now until everything queued has finished playing, so
// the caller can hold the microphone gate open for the whole answer and not
// just this piece.
pub fn play_sequenced(wav: Vec<u8>) -> Option<std::time::Duration> {
    let manager = MANAGER.get()?;
    let mut manager = manager.lock().ok()?;

    let data = match StaticSoundData::from_cursor(std::io::Cursor::new(wav)) {
        Ok(d) => d,
        Err(e) => {
            warn!("Speech chunk is not decodable audio: {}", e);
            return None;
        }
    };
    let millis = data.duration().as_millis() as u64;

    let mut guard = SEQUENCE.lock().ok()?;
    if guard.is_none() {
        // one clock for the life of the process. At 1000 ticks/s a u64 of
        // ticks lasts longer than the hardware, so it never needs resetting -
        // and resetting it would invalidate every start time already queued.
        let mut clock = match manager.add_clock(ClockSpeed::TicksPerSecond(1000.0)) {
            Ok(c) => c,
            Err(e) => {
                warn!("Cannot create the playback clock: {}", e);
                return None;
            }
        };
        clock.start();
        *guard = Some(Sequence { clock, next_tick: 0, playing: Vec::new() });
    }
    let seq = guard.as_mut()?;

    // drop clips that have finished so a long session does not accumulate
    // handles for every sentence ever spoken
    seq.playing.retain(|h| h.state() != PlaybackState::Stopped);

    let now = seq.clock.time().ticks;
    let starting = seq.next_tick == 0;

    let mut extra = EXTRA_PREROLL_MS.lock().map(|g| *g).unwrap_or(0);
    if starting {
        // a clean run earns a little of the head start back, so a one-off
        // hiccup does not tax every answer from now on
        extra = extra.saturating_sub(PREROLL_DECAY_MS);
        if let Ok(mut g) = EXTRA_PREROLL_MS.lock() {
            *g = extra;
        }
    }

    let earliest = now + if starting {
        FIRST_CHUNK_PREROLL_MS + extra
    } else {
        SCHEDULE_LEAD_MS
    };

    let start = if seq.next_tick > earliest {
        seq.next_tick
    } else {
        if !starting {
            // Synthesis fell behind playback. Logged rather than smoothed
            // over: it is the one thing that makes an answer sound broken and
            // it is invisible unless it is said out loud.
            let short_by = earliest.saturating_sub(seq.next_tick);
            let grown = (extra + short_by).min(PREROLL_MAX_MS);
            if let Ok(mut g) = EXTRA_PREROLL_MS.lock() {
                *g = grown;
            }
            warn!("Speech ran dry for {} ms - head start for the next answer is now {} ms",
                  short_by, FIRST_CHUNK_PREROLL_MS + grown);
        }
        earliest
    };

    let at = ClockTime { clock: seq.clock.id(), ticks: start, fraction: 0.0 };
    match manager.play(data.start_time(StartTime::ClockTime(at))) {
        Ok(handle) => {
            seq.playing.push(handle);
            seq.next_tick = start + millis;
            Some(std::time::Duration::from_millis(seq.next_tick.saturating_sub(now)))
        }
        Err(e) => {
            warn!("Failed to queue a speech chunk: {}", e);
            None
        }
    }
}

// Cut the answer short: stop what is sounding and forget what was scheduled.
pub fn stop_sequenced() {
    let Ok(mut guard) = SEQUENCE.lock() else { return };
    let Some(seq) = guard.as_mut() else { return };

    // a short fade rather than an instant cut, which clicks
    let tween = Tween { duration: std::time::Duration::from_millis(40), ..Default::default() };
    for handle in seq.playing.iter_mut() {
        handle.stop(tween);
    }
    seq.playing.clear();
    // the clock keeps running; only the running total is forgotten, so the
    // next answer schedules itself from now
    seq.next_tick = 0;
}