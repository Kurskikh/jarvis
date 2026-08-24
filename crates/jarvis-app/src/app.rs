use std::collections::VecDeque;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime};

use jarvis_core::{audio, audio_buffer::AudioRingBuffer, audio_processing, commands, config, listener, llm, recorder, speech, stt, intent, voices, ipc::{self, IpcEvent}, i18n, slots, DB};
use rand::seq::SliceRandom;

use crate::should_stop;

// VAD state machine
#[derive(Debug, Clone, Copy, PartialEq)]
enum VadState {
    WaitingForVoice,
    VoiceActive,
}

pub fn start(text_cmd_rx: Receiver<String>, rt: &tokio::runtime::Runtime) -> Result<(), ()> {
    // Before anything else: a previous run may have died while other
    // applications were turned down, and their volumes are not ours to leave
    // sitting there. The worker puts back anything it finds recorded.
    #[cfg(target_os = "windows")]
    jarvis_core::ducking::init();

    warm_speech(rt);
    main_loop(text_cmd_rx, rt)
}

// Get the speech sidecar up before anyone asks a question.
//
// Loading its model takes about ten seconds, and starting one takes longer
// still. Doing that lazily would put the whole delay in front of the first
// answer, after the "thinking" clip has already finished - the assistant would
// stand silent for the one turn where the owner is most likely to be watching.
//
// Spawned and not awaited: a sidecar that never comes up must delay nothing.
fn warm_speech(rt: &tokio::runtime::Runtime) {
    if !speech::is_enabled() {
        return;
    }
    let cfg = match speech::SpeechConfig::from_settings() {
        Ok(c) => c,
        Err(_) => return,
    };
    rt.spawn(async move {
        match speech::supervisor::ensure_running(&cfg).await {
            Ok(h) => info!("Speech ready: {} @ {} Hz", h.model, h.sample_rate.unwrap_or(0)),
            // not a warning: no sidecar is a normal way to run, the answers
            // simply stay written
            Err(e) => info!("Answers will not be spoken: {}", e),
        }
    });
}

fn main_loop(text_cmd_rx: Receiver<String>, rt: &tokio::runtime::Runtime) -> Result<(), ()> {
    let frame_length: usize = 512;
    let sample_rate: usize = 16000;
    let mut frame_buffer: Vec<i16> = vec![0; frame_length];

    // ring buffer: keeps last 5 seconds of audio (pre-roll)
    let mut audio_buffer = AudioRingBuffer::new(5.0, frame_length, sample_rate);

    // VAD state
    let mut vad_state = VadState::WaitingForVoice;
    let mut silence_frames: u32 = 0;

    // how many frames of silence before we consider speech ended
    // 1.5 seconds = 1.5 * (16000 / 512) ≈ 47 frames
    let silence_threshold: u32 = ((1.5 * sample_rate as f32) / frame_length as f32) as u32;

    voices::play_greet();

    match recorder::start_recording() {
        Ok(_) => info!("Recording started. Microphone: {}", 
            recorder::get_audio_device_name(recorder::get_selected_microphone_index())),
        Err(_) => {
            error!("Cannot start recording.");
            return Err(());
        }
    }

    ipc::send(IpcEvent::Idle);

    // ### WAKE WORD DETECTION LOOP
    'wake_word: loop {
        if should_stop() {
            info!("Stop signal received, shutting down...");
            voices::play_goodbye();
            ipc::send(IpcEvent::Stopping);
            break;
        }

        if let Ok(text) = text_cmd_rx.try_recv() {
            process_text_command(&text, &rt);
            continue 'wake_word;
        }

        recorder::read_microphone(&mut frame_buffer);

        // our own reaction is playing into this microphone; discard rather than
        // transcribe it, or the confirmation becomes the next "command"
        if audio::is_speaking() {
            continue;
        }
        let processed = audio_processing::process(&frame_buffer);

        match vad_state {
            VadState::WaitingForVoice => {
                // always buffer audio
                audio_buffer.push(&frame_buffer);

                if processed.is_voice {
                    // voice started! flush buffer to Vosk
                    info!("VAD: Voice started, flushing {} buffered frames", audio_buffer.len());

                    for buffered_frame in audio_buffer.drain_all() {
                        listener::data_callback(&buffered_frame);
                    }

                    vad_state = VadState::VoiceActive;
                    silence_frames = 0;
                }
            }

            VadState::VoiceActive => {
                // dual-feed: speech recognizer gets frames in parallel with wake word detector
                let _ = stt::recognize(&frame_buffer, false);

                // feed to wake word detector
                if let Some(_keyword_index) = listener::data_callback(&frame_buffer) {
                    // WAKE WORD DETECTED!
                    info!("Wake word activated!");
                    duck_others();
                    ipc::send(IpcEvent::WakeWordDetected);
                    // Answer NOW.
                    //
                    // This used to wait until the first transcript came back
                    // and was judged, which is a recogniser's schedule, not a
                    // conversation's: measured at 5.7 seconds between the
                    // detector firing and "да, сэр" being heard. The person
                    // sees the window react, hears nothing, and says the name
                    // again.
                    voices::play_reply();

                    stt::reset_wake_recognizer();
                    audio_processing::reset();

                    // brief sniff to keep feeding STT while transitioning
                    //
                    // The command recogniser still holds the wake word's own
                    // audio, and it finalises that on its own schedule -
                    // measured at 33 ms after activation on one utterance and
                    // 604 ms on the next. Either side of this window, that
                    // first finalised segment is the wake word rather than a
                    // command, and read_segment has to be told which segment
                    // that is.
                    //
                    // A result landing HERE is that segment, and it goes
                    // nowhere: this loop has no way to act on it. So record
                    // that it has been used up. Without this the rule would
                    // aim one segment too late and throw away the real command
                    // - which is precisely what a fast "джарвис" followed by a
                    // question would hit.
                    let sniff_frames = ((0.3 * sample_rate as f32) / frame_length as f32) as u32;
                    let mut wake_segment_pending = true;
                    for _ in 0..sniff_frames {
                        recorder::read_microphone(&mut frame_buffer);
                        audio_processing::process(&frame_buffer);
                        if stt::recognize(&frame_buffer, false).is_some() {
                            wake_segment_pending = false;
                        }
                    }
                    if !wake_segment_pending {
                        debug!("The wake word's own audio finalised during the sniff window");
                    }

                    ipc::send(IpcEvent::Listening);
                    recognize_command(&mut frame_buffer, &rt, frame_length, sample_rate,
                                      true, wake_segment_pending);

                    // The turn is over here, answer and all: recognize_command
                    // holds its window open while llm_busy() is true, which
                    // covers the model thinking and the answer being spoken.
                    unduck_others();

                    // reset state after command
                    vad_state = VadState::WaitingForVoice;
                    silence_frames = 0;
                    audio_buffer.clear();
                    stt::reset_wake_recognizer();
                    stt::reset_speech_recognizer(); // NOW reset, after command is done
                    audio_processing::reset();
                    ipc::send(IpcEvent::Idle);

                    continue 'wake_word;
                }

                // track silence
                if processed.is_voice {
                    silence_frames = 0;
                } else {
                    silence_frames += 1;

                    if silence_frames > silence_threshold {
                        debug!("VAD: Silence timeout, returning to wait state");
                        vad_state = VadState::WaitingForVoice;
                        silence_frames = 0;
                        stt::reset_wake_recognizer();
                        stt::reset_speech_recognizer(); // reset since we were dual-feeding
                    }
                }
            }
        }
    }

    recorder::stop_recording().ok();
    ipc::send(IpcEvent::Stopping);

    Ok(())
}


// Voice recognition for command after wake word
// What a finalized transcript means, given whether it is the first one after
// the wake word fired.
#[derive(Debug, PartialEq)]
enum Segment {
    // The wake word's own audio, coming back out of the full-vocabulary
    // recogniser. Nothing was asked: acknowledge and keep listening.
    WakeEcho,
    // The wake word on its own, said again mid-turn. Start over.
    Reactivate,
    // Run this.
    Command(String),
}

// The wake detector and the command recogniser are fed the same frames, so the
// first transcript after an activation is a decode of the very audio the
// detector just matched - the wake word itself, plus whatever was said in the
// same breath. Telling those two apart is this function's whole job.
//
// The detector reaches its verdict with a grammar of eight words, so it always
// spells the wake word the same way. The command recogniser has the entire
// language to choose from and spells that same audio however it likes: real
// examples off the logs are "баржа", "карлос", "райс" and "прорыв", with
// "борджа" and "каррас" as runners-up. The old test - "does the transcript
// contain a wake phrase?" - therefore failed exactly when the audio was hardest
// to decode, and a mishearing of the wake word went off to the language model
// as if it were a question. That is jarvis answering something nobody asked.
//
// So on the first segment the absence of the wake word is not evidence that a
// command was spoken; it is evidence that this decode did not understand the
// audio. A decode that lost the wake word cannot be trusted to have kept a
// command either, so it is dropped whole and the next segment is awaited. That
// costs a repeat when a run-on phrase is misheard - where today a corrupted
// prompt is sent instead - and it makes a phantom question impossible.
//
// Later segments are ordinary commands and carry no wake word, which is why the
// rule applies only to the one segment the detector matched.
// Remove these phrases from the text, matching WHOLE WORDS only.
//
// String::replace works on substrings, and the lists this serves are ordinary
// words that live inside longer ones. "скажи" is on the removal list, so
// "расскажи анекдот" came out as "рас анекдот" and the person heard their own
// question answered with a syllable missing. The English list is worse:
// "hey" is inside "they", "say" inside "essay", "tell" inside "intelligent".
//
// Longest match wins, so "слушаю сэр" is taken as one phrase rather than being
// half-eaten by the "сэр" entry that follows it.
fn strip_phrases(text: &str, phrases: &[&str]) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut kept: Vec<&str> = Vec::with_capacity(words.len());
    let mut i = 0;

    while i < words.len() {
        let mut matched = 0usize;

        for phrase in phrases {
            let p: Vec<&str> = phrase.split_whitespace().collect();
            if p.is_empty() || p.len() <= matched || i + p.len() > words.len() {
                continue;
            }
            if words[i..i + p.len()] == p[..] {
                matched = p.len();
            }
        }

        if matched > 0 {
            i += matched;
        } else {
            kept.push(words[i]);
            i += 1;
        }
    }

    kept.join(" ")
}

fn read_segment(
    text: &str,
    wake_phrases: &[&str],
    first_after_wake: bool,
    // Did this transcript settle too quickly to have had speech in it? See the
    // note on the single-word rule below.
    settled_fast: bool,
) -> Segment {
    let text = text.trim().to_lowercase();

    // whole words, so a name buried inside another word is not carved out of it
    let rest = strip_phrases(&text, wake_phrases);

    if rest != text {
        return if rest.is_empty() {
            if first_after_wake { Segment::WakeEcho } else { Segment::Reactivate }
        } else {
            Segment::Command(rest)
        };
    }

    // No name in the text, and only ONE WORD of that is suspicious enough to
    // throw away.
    //
    // Every phantom this has produced was a single word: "баржа", "карлос",
    // "райс", "прорыв", "жарвиз" - the name mangled by a full-vocabulary
    // decoder with nothing else in the audio to work with. But "джарвис, что
    // такое лес" said in one breath arrives as "что такое лес", because the
    // command recogniser is under no obligation to transcribe a name the
    // DETECTOR heard, and T-one routinely does not. Discarding that threw the
    // question away with it and left the person repeating themselves.
    // ...and only when it also settled too FAST to have been speech.
    //
    // One word alone is not enough evidence. "джарвис, как дела" reached this
    // point as the single token "какдела" - T-one writes it without the space -
    // and was thrown away, so the person asked twice and heard nothing the
    // first time. What separates that from a phantom is the clock: the
    // recogniser ends a phrase after a set silence, so a transcript containing
    // only the name lands about that long after the detector fired, while
    // anything with a question in it lands later. Measured off the logs: the
    // junk "с" arrived 1.2s after activation, "какдела" 4.2s.
    if first_after_wake && settled_fast && text.split_whitespace().count() <= 1 {
        return Segment::WakeEcho;
    }
    Segment::Command(text)
}

// What the microphone is listening FOR.
//
// Commands is the ordinary state: every phrase is matched against the packs,
// and only what fits nothing reaches the language model. Dialogue matches
// nothing at all - each phrase goes straight to the model, and the only thing
// examined first is whether it was a way of saying goodbye.
//
// Matching commands inside a dialogue was considered and left out on purpose.
// A conversation is full of sentences that resemble commands, and executing one
// mid-thought is worse than not having them.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Commands,
    Dialogue,
}

// What a turn asks the listening loop to do next.
#[derive(Clone, Copy, PartialEq, Debug)]
enum TurnOutcome {
    // stop after this, subject to the follow-up window
    Done,
    // a chaining command: keep taking commands
    Chain,
    // hand the microphone to the dialogue
    EnterDialogue,
}

// Was that a way of ending the conversation?
//
// Whole-phrase, not contained-in: "хватит об этом, расскажи про другое" is
// steering a conversation, not leaving one, and searching for "хватит" inside
// it would hang up on someone mid-sentence.
fn is_farewell(text: &str, phrases: &[&str]) -> bool {
    let text = text.trim();
    phrases.iter().any(|p| text == *p)
}

fn recognize_command(
    frame_buffer: &mut [i16],
    rt: &tokio::runtime::Runtime,
    frame_length: usize,
    sample_rate: usize,
    prefed_audio: bool,
    // Is the wake word's own audio still waiting to come out of the recogniser?
    // Separate from prefed_audio: that one says where this listening window
    // starts, this one says whether the first segment out of it belongs to the
    // wake word or to the person.
    wake_segment_pending: bool,
) {
    let mut audio_buffer = AudioRingBuffer::new(2.0, frame_length, sample_rate);
    let mut vad_state = if prefed_audio {
        VadState::VoiceActive
    } else {
        VadState::WaitingForVoice
    };
    let mut silence_frames: u32 = 0;
    let mut start = SystemTime::now();
    let mut first_recognition = wake_segment_pending;

    // how long this listening window lasts. It shortens to the follow-up
    // setting once a turn has been answered: waiting the full command timeout
    // again would leave the microphone open far longer than anyone expects
    // after an answer nobody followed up on.
    let mut deadline = config::CMS_WAIT_DELAY;
    let follow_up = std::time::Duration::from_secs(
        DB.get().map(|db| db.read().follow_up_secs).unwrap_or(0));

    // Commands until something says otherwise. The dialogue is entered from
    // inside this same loop rather than by calling a second one: the microphone,
    // the voice detector and the recogniser are all already set up here, and a
    // parallel loop would be the same eighty lines with a different ending.
    let mut mode = Mode::Commands;
    let dialogue_exit = std::time::Duration::from_secs(
        DB.get().map(|db| db.read().dialogue_exit_secs)
            .unwrap_or(config::DEFAULT_DIALOGUE_EXIT_SECS));

    // How soon after the activation a transcript can still be the name's own
    // echo rather than something a person said.
    //
    // The recogniser ends a phrase after this much silence, so a transcript
    // holding only the name arrives about that long after the detector fired.
    // The margin covers the detector's own lag - it reaches its verdict near
    // the END of the name, not at its start.
    let echo_window = std::time::Duration::from_millis(
        DB.get().map(|db| db.read().speech_pause_ms as u64).unwrap_or(800) + 700);

    // longer silence threshold for commands (user might pause to think)
    // 5 seconds
    let silence_threshold: u32 = ((5.0 * sample_rate as f32) / frame_length as f32) as u32;

    loop {
        if crate::should_stop() {
            return;
        }

        recorder::read_microphone(frame_buffer);

        // our own reaction is playing into this microphone; discard rather than
        // transcribe it, or the confirmation becomes the next "command"
        //
        // the clock is held back for as long as this lasts. An answer takes
        // seconds to arrive and another ten to read out, and a window that
        // counted down through all of it would be over before the assistant
        // stopped talking - the follow-up seconds are meant to start when
        // there is finally silence to speak into.
        if llm_busy() {
            start = SystemTime::now();
            silence_frames = 0;
            continue;
        }
        let processed = audio_processing::process(frame_buffer);

        match vad_state {
            VadState::WaitingForVoice => {
                audio_buffer.push(frame_buffer);

                if processed.is_voice {
                    // flush buffer to STT
                    for buffered_frame in audio_buffer.drain_all() {
                        stt::recognize(&buffered_frame, false);
                    }
                    vad_state = VadState::VoiceActive;
                    silence_frames = 0;
                } else {
                    silence_frames += 1;

                    if silence_frames > silence_threshold {
                        info!("Long silence detected, returning to wake word mode.");
                        return;
                    }
                }
            }

            VadState::VoiceActive => {
                // feed to STT
                if let Some((mut recognized_voice, confidence)) =
                    stt::recognize_command(frame_buffer)
                {
                    // the score is reported, not enforced. Vosk's confidence is
                    // a summed log-likelihood: it grows with the length of the
                    // utterance, so "above 120" means one thing for a word and
                    // another for a sentence, and a threshold picked without
                    // real samples of both would reject good commands to catch
                    // bad ones. Logged so that threshold can be chosen from
                    // measurements instead of taste.
                    info!("Recognized voice: {} (confidence {:.1})",
                          recognized_voice, confidence);

                    ipc::send(IpcEvent::SpeechRecognized {
                        text: recognized_voice.clone(),
                    });
                    supersede_llm_turn();

                    recognized_voice = recognized_voice.to_lowercase();

                    // measured from the moment this listening window opened,
                    // which for the first segment is the activation itself
                    let settled_fast = start
                        .elapsed()
                        .map(|since_wake| since_wake < echo_window)
                        .unwrap_or(false);

                    match read_segment(
                        &recognized_voice,
                        config::get_wake_phrases(&i18n::get_language()),
                        first_recognition,
                        settled_fast,
                    ) {
                        Segment::WakeEcho => {
                            info!("Discarding the wake word's own audio, heard as '{}'",
                                  recognized_voice);
                            first_recognition = false;
                            stt::reset_speech_recognizer();
                            // no reply here any more - it was said when the
                            // wake word landed
                            vad_state = VadState::WaitingForVoice;
                            silence_frames = 0;
                            start = SystemTime::now();
                            audio_buffer.clear();
                            continue;
                        }
                        Segment::Reactivate => {
                            info!("Wake word repeated during chaining, reactivating...");
                            voices::play_reply();
                            stt::reset_speech_recognizer();
                            ipc::send(IpcEvent::Listening);

                            vad_state = VadState::WaitingForVoice;
                            silence_frames = 0;
                            start = SystemTime::now();
                            audio_buffer.clear();
                            continue;
                        }
                        Segment::Command(text) => {
                            if text != recognized_voice {
                                info!("Wake word + command in one phrase: '{}'", text);
                            }
                            recognized_voice = text;
                        }
                    }

                    first_recognition = false;

                    // Two texts from here on, and the difference matters.
                    //
                    // The politeness words come off for MATCHING, because the
                    // command packs are keyed on the verb and "скажи погоду"
                    // should reach the same command as "погода". They must NOT
                    // come off the question put to the language model: asked
                    // "расскажи анекдот" it was handed "анекдот" and answered a
                    // word instead of a person. What someone said is what they
                    // get answered.
                    recognized_voice = recognized_voice.trim().to_string();

                    let mut for_matching = strip_phrases(
                        &recognized_voice,
                        config::get_phrases_to_remove(&i18n::get_language()),
                    );

                    // a phrase made entirely of these words still deserves a reply
                    if for_matching.is_empty() {
                        for_matching = recognized_voice.clone();
                    }
                    if for_matching != recognized_voice {
                        debug!("Matching on '{}' (said: '{}')",
                               for_matching, recognized_voice);
                    }

                    // CHARACTERS, not bytes. String::len() counts bytes, so the
                    // old "< 5" was about two Cyrillic letters - a filter that
                    // barely existed for the one language this ships with.
                    //
                    // The bound stays low on purpose. It cannot be raised to
                    // catch a four-letter mishearing, because the shortest
                    // command actually shipped is "всё" at three characters:
                    // any threshold that rejects the noise rejects the command
                    // too. Length is a floor against fragments, not a quality
                    // check - see MIN_COMMAND_CHARS and the test that pins it
                    // to the shipped packs.
                    if recognized_voice.chars().count() < MIN_COMMAND_CHARS {
                        debug!("Ignoring too short recognition: '{}'", recognized_voice);
                        continue;
                    }

                    // Inside a dialogue the packs are never consulted. The
                    // only thing looked at first is whether this was goodbye.
                    if mode == Mode::Dialogue {
                        if is_farewell(
                            &for_matching,
                            config::get_dialogue_exit_phrases(&i18n::get_language()),
                        ) {
                            info!("Dialogue ended by '{}'.", recognized_voice);
                            voices::play_goodbye();
                            forget_conversation();
                            ipc::send(IpcEvent::Idle);
                            return;
                        }

                        ask_the_model(&recognized_voice, rt, Announce::Quietly);

                        stt::reset_speech_recognizer();
                        vad_state = VadState::WaitingForVoice;
                        silence_frames = 0;
                        start = SystemTime::now();
                        audio_buffer.clear();
                        ipc::send(IpcEvent::Listening);
                        continue;
                    }

                    // execute command and check if we should chain
                    let outcome =
                        execute_command(&recognized_voice, &for_matching, rt);

                    if outcome == TurnOutcome::EnterDialogue {
                        info!("Dialogue started. It ends after {}s of silence, \
                               or when told to.", dialogue_exit.as_secs());
                        mode = Mode::Dialogue;
                        deadline = dialogue_exit;
                        stt::reset_speech_recognizer();
                        vad_state = VadState::WaitingForVoice;
                        silence_frames = 0;
                        start = SystemTime::now();
                        audio_buffer.clear();
                        ipc::send(IpcEvent::Listening);
                        continue;
                    }

                    if outcome == TurnOutcome::Chain {
                        // chain: reset and continue listening
                        info!("Chaining enabled, continuing to listen...");
                        stt::reset_speech_recognizer();
                        vad_state = VadState::WaitingForVoice;
                        silence_frames = 0;
                        start = SystemTime::now();
                        audio_buffer.clear();
                        ipc::send(IpcEvent::Listening);
                        continue;
                    } else if !follow_up.is_zero() {
                        // Keep listening for a while so a conversation does not
                        // need the wake word before every sentence. The window
                        // does not start counting until the assistant has
                        // finished answering - see the llm_busy check above.
                        info!("Listening for a follow-up ({}s after the answer)...",
                              follow_up.as_secs());
                        stt::reset_speech_recognizer();
                        vad_state = VadState::WaitingForVoice;
                        silence_frames = 0;
                        start = SystemTime::now();
                        deadline = follow_up;
                        audio_buffer.clear();
                        ipc::send(IpcEvent::Listening);
                        continue;
                    } else {
                        // no chain: return to wake word
                        info!("No chain, returning to wake word mode.");
                        return;
                    }
                }

                // track silence
                if processed.is_voice {
                    silence_frames = 0;
                } else {
                    silence_frames += 1;

                    if silence_frames > silence_threshold {
                        info!("Long silence detected, returning to wake word mode.");
                        return;
                    }
                }
            }
        }

        // timeout
        if let Ok(elapsed) = start.elapsed() {
            if elapsed > deadline {
                if mode == Mode::Dialogue {
                    // counted from the end of the answer, not the question -
                    // the llm_busy check above holds the clock while speaking
                    info!("Dialogue ended after {}s of silence.", deadline.as_secs());
                    voices::play_goodbye();
                    forget_conversation();
                    ipc::send(IpcEvent::Idle);
                } else {
                    info!("Nothing said for {}s, returning to wake word mode.",
                          deadline.as_secs());
                }
                return;
            }
        }
    }
}


fn process_text_command(text: &str, rt: &tokio::runtime::Runtime) {
    info!("Processing text command: {}", text);

    ipc::send(IpcEvent::SpeechRecognized { text: text.to_string() });
    supersede_llm_turn();

    // the same two texts as the spoken path - see the note there
    let spoken = text.to_lowercase().trim().to_string();

    if spoken.is_empty() {
        ipc::send(IpcEvent::Idle);
        return;
    }

    let mut filtered = strip_phrases(
        &spoken,
        config::get_phrases_to_remove(&i18n::get_language()),
    );

    // a line made entirely of these words still deserves a reply
    if filtered.is_empty() {
        filtered = spoken.clone();
    }

    // text commands never chain
    //
    // A dialogue cannot start from here either: it is a state of the MICROPHONE,
    // and the typed box has none to hand over. Saying so is better than the
    // command appearing to do nothing.
    if execute_command(&spoken, &filtered, rt) == TurnOutcome::EnterDialogue {
        info!("A dialogue can only be started by voice; nothing to listen with here.");
    }
}


// Execute command, returns true if chaining should continue
//
// `spoken` is what the person actually said; `text` is that with the politeness
// words taken off, which is what the command packs are matched against. They
// differ only when something was stripped, and the language model is always
// given `spoken` - see the note at the call site.
fn execute_command(
    spoken: &str,
    text: &str,
    rt: &tokio::runtime::Runtime,
) -> TurnOutcome {
    // one Arc snapshot per utterance. the &JCommand borrowed below outlives the
    // whole command execution, so it must not come from a lock guard
    let commands_list = jarvis_core::commands_list();

    if commands_list.is_empty() {
        ipc::send(IpcEvent::Error { message: "Commands not loaded".to_string() });
        ipc::send(IpcEvent::Idle);
        return TurnOutcome::Done;
    }

    // the intent branch is tried first, but it must not be able to SWALLOW the
    // utterance: a classifier trained on a command list that has since changed
    // can return an id nothing in the current list answers to, and taking that
    // as "not found" drops the phrase instead of trying the fuzzy matcher.
    let cmd_result = match rt.block_on(intent::classify(text)) {
        Some((intent_id, confidence)) => {
            info!("Intent recognized: {} (confidence: {:.2})", intent_id, confidence);

            match intent::get_command_by_intent(&commands_list, &intent_id) {
                Some(found) => Some(found),
                None => {
                    warn!("Intent '{}' does not resolve to a command, \
                           trying levenshtein fallback...", intent_id);
                    commands::fetch_command(text, &commands_list)
                }
            }
        }
        None => {
            info!("Intent not recognized, trying levenshtein fallback...");
            commands::fetch_command(text, &commands_list)
        }
    };

    if let Some((cmd_path, cmd_config)) = cmd_result {
        info!("Command found: {:?}", cmd_path);

        // extract slots if needed
        let extracted_slots = if !cmd_config.slots.is_empty() {
            let s = slots::extract(text, &cmd_config.slots);
            if !s.is_empty() {
                info!("Extracted slots: {:?}", s);
            }
            Some(s)
        } else {
            None
        };

        match commands::execute_command(&cmd_path, &cmd_config, Some(&text), extracted_slots.as_ref()) {
            Ok(chain) => {
                info!("Command executed successfully");
                // A command that ends the chain ends the conversation with it.
                // That is what "стоп" or "хватит" means said out loud: not
                // merely stop listening, but drop the thread - the reason for
                // saying it is usually that the thread has gone somewhere
                // useless, and carrying it into the next question would carry
                // the problem with it.
                //
                // Nothing is said or stopped here: every recognition already
                // ran supersede_llm_turn, so an answer in flight is cancelled
                // and its speech silenced before this point.
                if !chain {
                    forget_conversation();
                }
                // voices::play_ok();
                voices::play_random_from(cmd_config.get_sounds(&i18n::get_language()).as_slice());
                ipc::send(IpcEvent::CommandExecuted {
                    id: cmd_config.id.clone(),
                    success: true,
                });
                ipc::send(IpcEvent::Idle);

                // The dialogue type runs nothing; its whole effect is here, on
                // what the listening loop does next.
                return if cmd_config.cmd_type == "dialogue" {
                    TurnOutcome::EnterDialogue
                } else if chain {
                    TurnOutcome::Chain
                } else {
                    TurnOutcome::Done
                };
            }
            Err(msg) => {
                error!("Error executing command: {}", msg);
                voices::play_error();
                ipc::send(IpcEvent::CommandExecuted {
                    id: cmd_config.id.clone(),
                    success: false,
                });
                ipc::send(IpcEvent::Error { message: msg.to_string() });
            }
        }
    } else {
        info!("No command found for: {}", text);

        // From here on it is `spoken`, not `text`. The command packs are done
        // with; what remains is a person asking a question, and they asked it
        // in their own words.

        ask_the_model(spoken, rt, Announce::Aloud);
    }

    ipc::send(IpcEvent::Idle);
    TurnOutcome::Done // no chain on error or not found
}

// Whether to fill the wait before an answer with a spoken placeholder.
//
// Outside a dialogue it is worth it: a local model takes seconds, and silence
// after a command reads as "it did not hear me". Inside one it is the opposite
// - a person who is talking rather than commanding hears "думаю над ответом"
// before every sentence and the conversation stops being one.
#[derive(Clone, Copy, PartialEq)]
enum Announce {
    Aloud,
    Quietly,
}

// Put a question to the language model.
//
// The "I did not understand that" slot outside a dialogue, and the whole of
// every turn inside one.
fn ask_the_model(spoken: &str, rt: &tokio::runtime::Runtime, announce: Announce) {
    // is_enabled() first, and from_settings() only inside the arm: this is the
    // audio thread, and the default (disabled) path must not take the settings
    // read lock and clone five Strings on every no-match.
    if !llm::is_enabled() {
        // switched off: exactly as before this stage existed
        voices::play_not_found();
        ipc::send(IpcEvent::Error {
            message: format!("Command not found: {}", spoken)
        });
        return;
    }

    match llm::LlmConfig::from_settings() {
        Ok(cfg) => {
            // not_found.wav normally does NOT play here. it is a canned "no",
            // and with no TTS in this stage that sound plus the GUI IS the
            // whole reply - announcing failure and then quietly putting an
            // answer on screen contradicts itself.
            //
            // UNLESS nobody is subscribed to the IPC broadcast: then the
            // answer, and any error explaining its absence, is dropped by
            // ipc::send and the turn produces no observable output at all.
            // running with the GUI closed is a supported state (tray.rs
            // open_settings checks the same thing), so in it the old audible
            // reaction is still the whole reply.
            if !ipc::has_clients() && announce == Announce::Aloud {
                debug!("No IPC client attached; the LLM answer has nowhere to land.");
                voices::play_not_found();
            }
            spawn_llm_turn(rt, cfg, spoken.to_string(), announce);
        }
        // switched on but unusable: no model name, or a remote endpoint with
        // llm_allow_remote off. old behaviour, plus the reason on the wire - a
        // half-configured LLM that is silent is unfixable.
        Err(e) => {
            warn!("LLM turn skipped: {}", e);
            voices::play_not_found();
            ipc::send(IpcEvent::LlmAnswer {
                request_id: String::new(),
                prompt: spoken.to_string(),
                answer: None,
                model: String::new(),
                elapsed_ms: 0,
                error_code: Some(e.code().to_string()),
                error: Some(e.to_string()),
            });
        }
    }
}


// ### LLM TURN (stage 1: text answer only - no tools, no streaming, no speech)

// generation of the newest LLM turn. the request is spawned and the utterance
// returns immediately, so two quick "no command found"s put two answers in
// flight; without this the slower one lands last and overwrites the newer
// question's answer in the GUI.
// Shorter than this and it cannot be anything anyone meant to say.
//
// Three, because the shortest phrase in the shipped command packs is "всё".
// Deliberately NOT tuned upward to reject mishearings: "райс" - a real false
// positive from this microphone - is four characters, longer than a command
// that has to keep working. Sorting those two apart needs recognition
// confidence, not length. The test at the bottom of this file keeps the
// constant honest against the packs.
const MIN_COMMAND_CHARS: usize = 3;

// What the assistant remembers of the conversation.
//
// A chat window has a "new conversation" button. A voice has nothing of the
// kind, so a thread that is never ended is a thread that lasts until the
// process does - and tomorrow morning's question would be read in the light of
// tonight's, with no way for the person asking to tell that is what happened.
//
// So it ends two ways. The stop word clears it outright, and it lapses on its
// own after a stretch of silence. Both are needed: the first is for a thread
// that has gone wrong, the second for one simply left behind.
struct Conversation {
    turns: VecDeque<llm::Exchange>,
    last: Option<Instant>,
}

impl Conversation {
    const fn new() -> Self {
        Conversation { turns: VecDeque::new(), last: None }
    }

    // What travels with the next question. Takes `now` rather than reading the
    // clock so the lapse can be tested without waiting for it.
    fn recall(&mut self, now: Instant, idle: Duration) -> Vec<llm::Exchange> {
        if let Some(last) = self.last {
            if now.duration_since(last) >= idle {
                debug!("The conversation lapsed after {:?} of silence", now.duration_since(last));
                self.clear();
                return Vec::new();
            }
        }
        self.turns.iter().cloned().collect()
    }

    // Only a complete exchange is worth keeping. A question with no answer
    // reads to the model as one the assistant ignored, and an answer with no
    // text teaches it that saying nothing is acceptable here.
    fn record(&mut self, now: Instant, user: String, assistant: String, depth: usize) {
        if user.trim().is_empty() || assistant.trim().is_empty() || depth == 0 {
            return;
        }
        self.turns.push_back(llm::Exchange { user, assistant });
        while self.turns.len() > depth {
            self.turns.pop_front();
        }
        self.last = Some(now);
    }

    fn clear(&mut self) {
        self.turns.clear();
        self.last = None;
    }
}

static CONVERSATION: once_cell::sync::Lazy<parking_lot::Mutex<Conversation>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(Conversation::new()));

// Depth and lapse, or None when remembering is switched off.
fn conversation_settings() -> Option<(usize, Duration)> {
    let db = DB.get()?;
    let s = db.read();
    if !s.llm_history {
        return None;
    }
    Some((
        s.llm_history_turns as usize,
        Duration::from_secs(s.llm_history_idle_min as u64 * 60),
    ))
}

// Quiet everything else for the length of a turn.
//
// Called on the wake word rather than when the answer starts, so the music is
// already out of the way while the command is being spoken - which is also why
// it helps the recogniser and not only the listener.
#[cfg(target_os = "windows")]
fn duck_others() {
    let Some(db) = DB.get() else { return };
    let (on, level) = {
        let s = db.read();
        (s.duck_others, s.duck_level)
    };
    if on {
        jarvis_core::ducking::duck(level as f32 / 100.0);
    }
}

// Put it back. Unconditional on purpose: if the setting was switched off in
// the middle of a turn, what is already ducked still has to come back up.
#[cfg(target_os = "windows")]
fn unduck_others() {
    jarvis_core::ducking::restore();
}

#[cfg(not(target_os = "windows"))]
fn duck_others() {}
#[cfg(not(target_os = "windows"))]
fn unduck_others() {}

// Forget the thread. Said out loud, or asked for from the window.
pub fn forget_conversation() {
    let mut c = CONVERSATION.lock();
    if !c.turns.is_empty() {
        debug!("Forgetting {} remembered exchange(s)", c.turns.len());
    }
    c.clear();
}

static LLM_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

// How many language model turns are still running - asking, or reading the
// answer out. A counter and not a flag: superseding spawns the new turn before
// aborting the old one, so a flag would be cleared by the corpse of the turn
// that was just replaced.
static LLM_PENDING: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

// Decrements on drop, so an aborted turn releases the count too. Created
// synchronously and moved into the task, which covers the case where the
// future is dropped before it is ever polled.
struct PendingTurn;

impl Drop for PendingTurn {
    fn drop(&mut self) {
        LLM_PENDING.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

// true while an answer is still coming: being generated, or being spoken
fn llm_busy() -> bool {
    LLM_PENDING.load(std::sync::atomic::Ordering::SeqCst) > 0
        || audio::is_speaking()
}

// handle of the in-flight turn, so a new question also CANCELS the old request
// instead of leaving the model generating tokens nobody will read.
static LLM_TASK: once_cell::sync::Lazy<parking_lot::Mutex<Option<tokio::task::JoinHandle<()>>>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(None));

// retire whatever LLM turn is in flight, from the audio thread, on every new
// utterance - not only on the ones that reach spawn_llm_turn.
//
// without this an utterance that DOES match a command leaves the previous
// question's request running: the GUI clears its panel on speech_recognized
// (frontend/src/lib/ipc.ts), the generation counter never moves, and the late
// answer is then admitted onto a panel whose question is two turns old.
//
// no terminal event is sent for the dropped turn and none is needed: the
// speech_recognized that precedes this call has already cleared the panel, so
// there is no spinner left to strand.
fn supersede_llm_turn() {
    use std::sync::atomic::Ordering;

    LLM_GEN.fetch_add(1, Ordering::SeqCst);
    if let Some(old) = LLM_TASK.lock().take() {
        old.abort();
    }
    // and stop the previous answer mid-word if it is still being spoken.
    // Aborting the task alone would not: the audio is already queued in the
    // mixer and would keep playing over the new question.
    speech::stop();
}

// Cut short whatever the assistant is saying, from the tray or the window.
//
// Bumping the generation is what actually stops it: the speaking loop checks
// it between chunks and stops asking the sidecar for more, so the answer ends
// rather than merely going quiet while synthesis carries on.
pub fn stop_speaking() {
    use std::sync::atomic::Ordering;

    LLM_GEN.fetch_add(1, Ordering::SeqCst);
    speech::stop();
    debug!("Speech stopped by request");
}

// ask the LLM about an utterance no command matched, WITHOUT blocking here.
//
// this runs on the audio thread. recorder::read_microphone is a blocking pull
// and pv_recorder's driver-side ring is 50 frames = 1.6s at 512/16000, so a
// multi-second call here would first make the loop run behind wall clock and
// then start losing audio outright - the wake word would be deaf for the whole
// answer. stage 1's answer is text-only and is not part of the turn, so nothing
// downstream needs it: spawn and return.
//
// rt.spawn, NOT tokio::spawn: this thread is a plain std::thread (main.rs:221)
// with no reactor in context, and a bare tokio::spawn panics there - the mirror
// image of the hazard documented at main.rs:145-150. the future must be
// 'static, hence the owned cfg and the cloned prompt.
fn spawn_llm_turn(
    rt: &tokio::runtime::Runtime,
    cfg: llm::LlmConfig,
    prompt: String,
    announce: Announce,
) {
    use std::sync::atomic::Ordering;

    let generation = LLM_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let request_id = generation.to_string();

    // spoken before the request goes out. a local model takes seconds and the
    // assistant would otherwise answer silence with silence - except in a
    // dialogue, where saying it every turn is worse than the wait.
    if announce == Announce::Aloud {
        voices::play_thinking();
    }

    // counted from here, not from inside the task: between spawn and the first
    // poll the future can be dropped, and a guard created in there would never
    // exist to be dropped
    LLM_PENDING.fetch_add(1, Ordering::SeqCst);
    let pending = PendingTurn;

    let handle = rt.spawn(async move {
        let _pending = pending;
        ipc::send(IpcEvent::LlmThinking {
            request_id: request_id.clone(),
            prompt: prompt.clone(),
        });

        // read once, before the call: the settings can change while a slow
        // answer is in flight, and a turn should finish under the rules it
        // started under
        let memory = conversation_settings();
        let history = match memory {
            Some((_, idle)) => CONVERSATION.lock().recall(Instant::now(), idle),
            None => Vec::new(),
        };
        if !history.is_empty() {
            debug!("Carrying {} remembered exchange(s) into this question", history.len());
        }
        let asked = prompt.clone();

        let started = std::time::Instant::now();
        let result = llm::ask(&cfg, &prompt, &history).await;
        let elapsed_ms = started.elapsed().as_millis() as u64;

        // a newer utterance started while this was in flight. its answer is the
        // one the user is waiting for; drop this one rather than race it.
        if LLM_GEN.load(Ordering::SeqCst) != generation {
            debug!("LLM answer for generation {} superseded, dropped", generation);
            return;
        }

        match result {
            Ok(a) => {
                // the synthesiser's tags are for the synthesiser. The window
                // and the log get prose; the sidecar gets a.text as written.
                let shown = speech::strip_markup(&a.text);
                // what the model is told it said is the prose that was
                // actually spoken, not the marked-up form
                let remembered = shown.clone();

                // the text goes in the log too, {:?} so a multi-line answer
                // stays one line. with no GUI attached (see the has_clients
                // check at the hook) the log is the only place it lands.
                info!("LLM answered in {} ms ({} completion tokens, model {}): {:?}",
                      elapsed_ms, a.completion_tokens, a.model, shown);
                ipc::send(IpcEvent::LlmAnswer {
                    request_id,
                    prompt,
                    answer: Some(shown),
                    model: a.model,
                    elapsed_ms,
                    error_code: None,
                    error: None,
                });

                // Remembered only once it is a real answer. A failed turn
                // and a superseded one leave nothing behind - the alternative
                // is a model reasoning from words it never said.
                if let Some((depth, _)) = memory {
                    CONVERSATION.lock().record(Instant::now(), asked, remembered, depth);
                }

                // the answer has already reached the window and the log; the
                // voice is on top of that, never instead of it
                speak_answer(&a.text, generation).await;
            }
            Err(e) => {
                warn!("LLM turn failed after {} ms: {}", elapsed_ms, e);
                ipc::send(IpcEvent::LlmAnswer {
                    request_id,
                    prompt,
                    answer: None,
                    model: cfg.model.clone(),
                    elapsed_ms,
                    error_code: Some(e.code().to_string()),
                    error: Some(e.to_string()),
                });
            }
        }
    });

    // cancelling the previous request drops its reqwest future, which aborts
    // the HTTP call and stops the server generating for a dead question
    if let Some(old) = LLM_TASK.lock().replace(handle) {
        old.abort();
    }
}


// Say an answer out loud, if there is anything to say it with.
//
// Every failure here is logged and swallowed. Speech is the last step of a
// turn that has already succeeded: the answer is on screen and in the log
// before this runs, so nothing about a missing or broken sidecar should look
// like the question failed.
async fn speak_answer(text: &str, generation: u64) {
    use std::sync::atomic::Ordering;

    if text.trim().is_empty() {
        return;
    }

    let cfg = match speech::SpeechConfig::from_settings() {
        Ok(c) => c,
        Err(speech::SpeechError::Disabled) => return,
        Err(e) => {
            debug!("Not speaking: {}", e);
            return;
        }
    };

    // cheap when it is already up, and it means a sidecar started after
    // Jarvis is picked up without restarting anything
    if let Err(e) = speech::supervisor::ensure_running(&cfg).await {
        warn!("Not speaking the answer: {}", e);
        return;
    }

    let superseded = || LLM_GEN.load(Ordering::SeqCst) != generation;
    match speech::say(&cfg, text, superseded).await {
        Ok(s) if s.cancelled =>
            debug!("Speech stopped after {} chunk(s)", s.frames),
        Ok(s) => info!(
            "Spoke the answer in {} chunk(s): first at {} ms, {} ms in total{}",
            s.frames, s.first_frame_ms, s.total_ms,
            if s.fell_back { " (sidecar fell back to one shot)" } else { "" }),
        Err(e) => warn!("Speaking the answer failed: {}", e),
    }
}

pub fn close(code: i32) {
    info!("Closing application.");
    voices::play_goodbye();
    ipc::send(IpcEvent::Stopping);
    // before the exit, not after: std::process::exit runs no destructors, and
    // a sidecar left behind holds both the port and the GPU while the next run
    // wonders what has them
    speech::supervisor::shutdown();
    // and the same for other applications' volumes - they are not ours to
    // leave turned down
    #[cfg(target_os = "windows")]
    jarvis_core::ducking::restore_blocking();
    std::process::exit(code);
}
#[cfg(test)]
mod length_floor_tests {
    use super::MIN_COMMAND_CHARS;

    // Every Russian phrase in the shipped command packs, straight off disk.
    fn shipped_phrases() -> Vec<String> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../resources/commands");
        let mut out = Vec::new();
        let entries = std::fs::read_dir(&root)
            .unwrap_or_else(|e| panic!("cannot read {}: {}", root.display(), e));
        for entry in entries.flatten() {
            let toml = entry.path().join("command.toml");
            let Ok(text) = std::fs::read_to_string(&toml) else { continue };
            // a deliberately dumb scan: this test exists to compare the
            // constant against reality, and pulling in a toml parser to do it
            // would let a parser quirk hide the very thing being checked
            let mut rest = text.as_str();
            while let Some(at) = rest.find("phrases.ru") {
                rest = &rest[at..];
                let Some(open) = rest.find('[') else { break };
                let Some(close) = rest[open..].find(']') else { break };
                let block = &rest[open..open + close];
                for piece in block.split('"').skip(1).step_by(2) {
                    out.push(piece.to_string());
                }
                rest = &rest[open + close..];
            }
        }
        assert!(out.len() > 50, "only found {} phrases - the scan is broken", out.len());
        out
    }

    // The floor must not reject anything the assistant is supposed to obey.
    // This is the constraint that stops the constant being raised to swat a
    // mishearing: the shortest shipped command is shorter than the noise.
    #[test]
    fn the_floor_lets_every_shipped_command_through() {
        let phrases = shipped_phrases();
        let shortest = phrases.iter()
            .min_by_key(|p| p.chars().count())
            .expect("no phrases");

        for p in &phrases {
            assert!(p.chars().count() >= MIN_COMMAND_CHARS,
                    "the length floor of {} would reject the shipped command {:?} \
                     ({} chars)", MIN_COMMAND_CHARS, p, p.chars().count());
        }

        // and the floor is not idly low: it sits right at the shortest command,
        // so raising it by one breaks something real
        assert_eq!(shortest.chars().count(), MIN_COMMAND_CHARS,
                   "the shortest shipped command is {:?}; MIN_COMMAND_CHARS should \
                    equal its length, not sit above or below it", shortest);
    }

    // The bug this replaced: String::len() counts bytes, so the old floor of 5
    // was about two Cyrillic letters and let four-letter noise straight through.
    #[test]
    fn the_floor_counts_characters_not_bytes() {
        assert_eq!("всё".chars().count(), 3);
        assert_eq!("всё".len(), 6, "Cyrillic is two bytes a letter - that was the bug");
        assert_eq!("райс".chars().count(), 4);
        assert!("райс".len() >= 5, "the old byte floor of 5 could never reject it");
    }
}

#[cfg(test)]
mod stripping_tests {
    use super::strip_phrases;

    // The list really does contain these, and they really do live inside
    // longer words. This is the bug: "расскажи анекдот" reached the language
    // model as "рас анекдот", and "скажи анекдот" as bare "анекдот".
    const RU_TBR: &[&str] = &[
        "джарвис", "сэр", "слушаю сэр",
        "произнеси", "ответь", "покажи", "скажи", "давай",
    ];

    #[test]
    fn a_word_that_merely_contains_one_is_left_whole() {
        assert_eq!(strip_phrases("расскажи анекдот", RU_TBR), "расскажи анекдот");
    }

    #[test]
    fn the_word_itself_still_comes_off() {
        assert_eq!(strip_phrases("скажи анекдот", RU_TBR), "анекдот");
    }

    #[test]
    fn the_longer_phrase_wins_over_the_shorter_one_inside_it() {
        // "сэр" sits inside "слушаю сэр"; taking the short one first would
        // leave "слушаю" stranded
        assert_eq!(strip_phrases("слушаю сэр открой музыку", RU_TBR), "открой музыку");
    }

    #[test]
    fn english_is_where_this_bites_hardest() {
        const EN: &[&str] = &["jarvis", "sir", "please", "say", "show", "tell", "hey"];
        assert_eq!(strip_phrases("they say it is an essay", EN), "they it is an essay");
        assert_eq!(strip_phrases("hey jarvis tell me", EN), "me");
    }

    #[test]
    fn a_phrase_of_nothing_but_these_words_comes_back_empty() {
        // the caller checks for this and falls back to what was said
        assert_eq!(strip_phrases("джарвис скажи", RU_TBR), "");
    }

    #[test]
    fn nothing_to_strip_leaves_the_text_alone() {
        assert_eq!(strip_phrases("открой музыку", RU_TBR), "открой музыку");
    }
}

#[cfg(test)]
mod farewell_tests {
    use super::is_farewell;

    const RU: &[&str] = &["стоп", "хватит", "всё", "давай закончим", "до свидания"];

    #[test]
    fn a_whole_phrase_ends_the_conversation() {
        for said in ["стоп", "хватит", "всё", "давай закончим", "до свидания"] {
            assert!(is_farewell(said, RU), "'{}' should end it", said);
        }
    }

    #[test]
    fn the_same_word_inside_a_sentence_does_not() {
        // this is the whole reason it is not a contains() check: both of these
        // are steering a conversation, not leaving one
        assert!(!is_farewell("хватит об этом расскажи про другое", RU));
        assert!(!is_farewell("всё это очень интересно", RU));
        assert!(!is_farewell("стоп машина это что за фильм", RU));
    }

    #[test]
    fn surrounding_space_is_not_a_reason_to_stay() {
        assert!(is_farewell("  хватит  ", RU));
    }

    #[test]
    fn an_ordinary_question_is_left_alone() {
        assert!(!is_farewell("расскажи анекдот", RU));
    }
}

#[cfg(test)]
mod wake_echo_tests {
    use super::{read_segment, Segment};

    const RU: &[&str] = &["джарвис", "джервис", "гарвис", "джарви", "гарви"];

    #[test]
    fn the_wake_word_alone_right_after_activation_is_its_own_echo() {
        assert_eq!(read_segment("джарвис", RU, true, true), Segment::WakeEcho);
    }

    #[test]
    fn a_mishearing_of_the_wake_word_is_dropped_rather_than_asked() {
        // every one of these is off a real log: the eight-word wake grammar
        // reported "джарвис" while the full vocabulary rendered the same audio
        // like this. Before this rule each went to the language model as a
        // question, and jarvis answered it out loud.
        for misheard in ["баржа", "карлос", "райс", "прорыв", "борджа", "каррас"] {
            assert_eq!(
                read_segment(misheard, RU, true, true),
                Segment::WakeEcho,
                "'{}' right after the wake word must not become a question",
                misheard
            );
        }
    }

    #[test]
    fn a_run_on_command_survives_without_the_name_in_it() {
        // "джарвис, что такое лес" in one breath. The detector heard the name;
        // the command recogniser had no obligation to transcribe it, and T-one
        // did not. Discarding this threw the question away.
        assert_eq!(
            read_segment("что такое лес", RU, true, true),
            Segment::Command("что такое лес".to_string())
        );
    }

    #[test]
    fn two_words_are_enough_to_be_taken_seriously() {
        assert_eq!(
            read_segment("открой музыку", RU, true, true),
            Segment::Command("открой музыку".to_string())
        );
    }

    // The bug: "джарвис, как дела" reaches this rule as the single token
    // "какдела" - T-one writes it without the space - and was thrown away, so
    // the question went unanswered and had to be asked twice. It arrived 4.2
    // seconds after the activation; the junk it was meant to catch arrives in
    // about one. Being short is no longer enough on its own.
    #[test]
    fn one_word_that_took_its_time_is_a_person_speaking() {
        assert_eq!(
            read_segment("какдела", RU, true, false),
            Segment::Command("какдела".to_string())
        );
    }

    #[test]
    fn one_word_that_arrived_at_once_is_still_the_echo() {
        assert_eq!(read_segment("с", RU, true, true), Segment::WakeEcho);
    }

    #[test]
    fn the_very_same_words_later_in_the_turn_are_ordinary_commands() {
        // the rule is about the ONE segment the detector matched. Afterwards
        // there is no wake word to expect, and a command must pass through - a
        // guard that swallowed these would make chaining useless.
        assert_eq!(
            read_segment("баржа", RU, false, true),
            Segment::Command("баржа".to_string())
        );
    }

    #[test]
    fn a_run_on_phrase_keeps_the_part_after_the_wake_word() {
        assert_eq!(
            read_segment("джарвис как дела", RU, true, true),
            Segment::Command("как дела".to_string())
        );
    }

    #[test]
    fn the_wake_word_alone_later_in_the_turn_starts_a_new_one() {
        assert_eq!(read_segment("джарвис", RU, false, true), Segment::Reactivate);
    }

    #[test]
    fn any_of_the_accepted_spellings_counts_as_the_wake_word() {
        // the detector matches fuzzily, so the command recogniser's near-misses
        // are listed as wake phrases too; all of them must strip the same way
        for spelling in RU {
            assert_eq!(
                read_segment(&format!("{} открой музыку", spelling), RU, true, true),
                Segment::Command("открой музыку".to_string()),
                "spelling '{}'",
                spelling
            );
        }
    }

    #[test]
    fn case_and_surrounding_space_do_not_matter() {
        assert_eq!(read_segment("  ДЖАРВИС  ", RU, true, true), Segment::WakeEcho);
    }
}

#[cfg(test)]
mod conversation_tests {
    use super::Conversation;
    use std::time::{Duration, Instant};

    fn ex(c: &Conversation, i: usize) -> (String, String) {
        (c.turns[i].user.clone(), c.turns[i].assistant.clone())
    }

    #[test]
    fn a_complete_exchange_is_kept() {
        let mut c = Conversation::new();
        c.record(Instant::now(), "какая погода".into(), "ясно".into(), 4);
        assert_eq!(c.turns.len(), 1);
        assert_eq!(ex(&c, 0), ("какая погода".to_string(), "ясно".to_string()));
    }

    #[test]
    fn a_half_exchange_is_not() {
        let mut c = Conversation::new();
        let now = Instant::now();
        c.record(now, "q".into(), "".into(), 4);
        c.record(now, "".into(), "a".into(), 4);
        c.record(now, "  ".into(), "  ".into(), 4);
        assert!(c.turns.is_empty());
        // and nothing was stamped, so an empty thread cannot "lapse"
        assert!(c.last.is_none());
    }

    #[test]
    fn only_the_last_few_are_carried() {
        let mut c = Conversation::new();
        let now = Instant::now();
        for i in 0..10 {
            c.record(now, format!("q{}", i), format!("a{}", i), 3);
        }
        assert_eq!(c.turns.len(), 3);
        // the oldest go, not the newest
        assert_eq!(ex(&c, 0).0, "q7");
        assert_eq!(ex(&c, 2).0, "q9");
    }

    #[test]
    fn the_thread_lapses_after_silence() {
        let mut c = Conversation::new();
        let start = Instant::now();
        c.record(start, "q".into(), "a".into(), 4);

        let idle = Duration::from_secs(300);
        assert_eq!(c.recall(start + Duration::from_secs(299), idle).len(), 1);
        assert!(c.recall(start + Duration::from_secs(300), idle).is_empty());
        // and it is gone, not merely withheld: the next question starts clean
        assert!(c.turns.is_empty());
    }

    #[test]
    fn each_answer_pushes_the_lapse_back() {
        // otherwise a long conversation would expire mid-sentence, counting
        // from whenever it happened to start
        let mut c = Conversation::new();
        let idle = Duration::from_secs(300);
        let start = Instant::now();
        c.record(start, "q1".into(), "a1".into(), 4);
        c.record(start + Duration::from_secs(200), "q2".into(), "a2".into(), 4);
        assert_eq!(c.recall(start + Duration::from_secs(400), idle).len(), 2);
    }

    #[test]
    fn the_stop_word_leaves_nothing_behind() {
        let mut c = Conversation::new();
        c.record(Instant::now(), "q".into(), "a".into(), 4);
        c.clear();
        assert!(c.turns.is_empty());
        assert!(c.last.is_none());
        // an empty thread must not lapse into anything odd
        assert!(c.recall(Instant::now(), Duration::from_secs(1)).is_empty());
    }

    #[test]
    fn a_depth_of_zero_remembers_nothing() {
        let mut c = Conversation::new();
        c.record(Instant::now(), "q".into(), "a".into(), 0);
        assert!(c.turns.is_empty());
    }
}
