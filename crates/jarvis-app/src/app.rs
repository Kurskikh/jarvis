use std::sync::mpsc::Receiver;
use std::time::SystemTime;

use jarvis_core::{audio, audio_buffer::AudioRingBuffer, audio_processing, commands, config, listener, llm, recorder, stt, intent, voices, ipc::{self, IpcEvent}, i18n, slots};
use rand::seq::SliceRandom;

use crate::should_stop;

// VAD state machine
#[derive(Debug, Clone, Copy, PartialEq)]
enum VadState {
    WaitingForVoice,
    VoiceActive,
}

pub fn start(text_cmd_rx: Receiver<String>, rt: &tokio::runtime::Runtime) -> Result<(), ()> {
    main_loop(text_cmd_rx, rt)
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
                    ipc::send(IpcEvent::WakeWordDetected);
                    
                    stt::reset_wake_recognizer();
                    audio_processing::reset();

                    // brief sniff to keep feeding STT while transitioning
                    let sniff_frames = ((0.3 * sample_rate as f32) / frame_length as f32) as u32;
                    for _ in 0..sniff_frames {
                        recorder::read_microphone(&mut frame_buffer);
                        audio_processing::process(&frame_buffer);
                        stt::recognize(&frame_buffer, false);
                    }

                    ipc::send(IpcEvent::Listening);
                    recognize_command(&mut frame_buffer, &rt, frame_length, sample_rate, true);

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
fn recognize_command(
    frame_buffer: &mut [i16],
    rt: &tokio::runtime::Runtime,
    frame_length: usize,
    sample_rate: usize,
    prefed_audio: bool
) {
    let mut audio_buffer = AudioRingBuffer::new(2.0, frame_length, sample_rate);
    let mut vad_state = if prefed_audio {
        VadState::VoiceActive
    } else {
        VadState::WaitingForVoice
    };
    let mut silence_frames: u32 = 0;
    let mut start = SystemTime::now();
    let mut first_recognition = prefed_audio;
    
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
        if audio::is_speaking() {
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
                if let Some(mut recognized_voice) = stt::recognize(frame_buffer, false) {
                    info!("Recognized voice: {}", recognized_voice);
                    
                    ipc::send(IpcEvent::SpeechRecognized {
                        text: recognized_voice.clone(),
                    });
                    supersede_llm_turn();
                    
                    recognized_voice = recognized_voice.to_lowercase();
                    
                    // check if wake word repeated (reactivate)
                    let wake_phrases = config::get_wake_phrases(&i18n::get_language());
                    let contains_wake = wake_phrases.iter().any(|wp| recognized_voice.contains(wp));

                    if contains_wake {
                        // strip the wake word
                        let mut remaining = recognized_voice.clone();
                        for wp in wake_phrases {
                            remaining = remaining.replace(wp, "");
                        }
                        let remaining = remaining.trim();

                        if remaining.is_empty() {
                            if first_recognition {
                                // leftover wake word from dual-feed, just discard it
                                info!("Discarding initial wake word from prefed audio");
                                first_recognition = false;
                                stt::reset_speech_recognizer();
                                voices::play_reply();
                                vad_state = VadState::WaitingForVoice;
                                silence_frames = 0;
                                start = SystemTime::now();
                                audio_buffer.clear();
                                continue;
                            }

                            // just wake word, no command - reactivate
                            info!("Wake word repeated during chaining, reactivating...");
                            voices::play_reply();
                            stt::reset_speech_recognizer();
                            ipc::send(IpcEvent::Listening);
                            
                            vad_state = VadState::WaitingForVoice;
                            silence_frames = 0;
                            start = SystemTime::now();
                            audio_buffer.clear();
                            continue;
                        } else {
                            // wake word + command in one phrase - execute the command part
                            info!("Wake word + command during chaining: '{}'", remaining);
                            recognized_voice = remaining.to_string();
                            // fall through to command execution below
                        }
                    }

                    first_recognition = false;
                    
                    // filter activation phrases
                    // for tbr in config::ASSISTANT_PHRASES_TBR {
                    //     recognized_voice = recognized_voice.replace(tbr, "");
                    // }
                    for tbr in config::get_phrases_to_remove(&i18n::get_language()) {
                        recognized_voice = recognized_voice.replace(tbr, "");
                    }

                    recognized_voice = recognized_voice.trim().to_string();
                    
                    if recognized_voice.len() < 5 {
                        debug!("Ignoring too short recognition: '{}'", recognized_voice);
                        continue;
                    }

                    if recognized_voice.is_empty() {
                        continue;
                    }
                    
                    // execute command and check if we should chain
                    let should_chain = execute_command(&recognized_voice, rt);
                    
                    if should_chain {
                        // chain: reset and continue listening
                        info!("Chaining enabled, continuing to listen...");
                        stt::reset_speech_recognizer();
                        vad_state = VadState::WaitingForVoice;
                        silence_frames = 0;
                        start = SystemTime::now();
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
            if elapsed > config::CMS_WAIT_DELAY {
                info!("Command timeout, returning to wake word mode.");
                return;
            }
        }
    }
}


fn process_text_command(text: &str, rt: &tokio::runtime::Runtime) {
    info!("Processing text command: {}", text);
    
    ipc::send(IpcEvent::SpeechRecognized { text: text.to_string() });
    supersede_llm_turn();
    
    let mut filtered = text.to_lowercase();
    // for tbr in config::ASSISTANT_PHRASES_TBR {
    //     filtered = filtered.replace(tbr, "");
    // }
    for tbr in config::get_phrases_to_remove(&i18n::get_language()) {
        filtered = filtered.replace(tbr, "");
    }

    let filtered = filtered.trim();
    
    if filtered.is_empty() {
        ipc::send(IpcEvent::Idle);
        return;
    }
    
    // text commands never chain
    execute_command(filtered, rt);
}


// Execute command, returns true if chaining should continue
fn execute_command(text: &str, rt: &tokio::runtime::Runtime) -> bool {
    // one Arc snapshot per utterance. the &JCommand borrowed below outlives the
    // whole command execution, so it must not come from a lock guard
    let commands_list = jarvis_core::commands_list();

    if commands_list.is_empty() {
        ipc::send(IpcEvent::Error { message: "Commands not loaded".to_string() });
        ipc::send(IpcEvent::Idle);
        return false;
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
                // voices::play_ok();
                voices::play_random_from(cmd_config.get_sounds(&i18n::get_language()).as_slice());
                ipc::send(IpcEvent::CommandExecuted {
                    id: cmd_config.id.clone(),
                    success: true,
                });
                ipc::send(IpcEvent::Idle);
                return chain; // return chain status from command
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

        // the LLM turn takes over the "I did not understand that" slot when it
        // is on AND usable. everything else is byte-for-byte the old behaviour.
        //
        // is_enabled() first, and from_settings() only inside the arm: this is
        // the audio thread, and the default (disabled) path must not take the
        // settings read lock and clone five Strings on every no-match.
        if !llm::is_enabled() {
            // switched off: exactly as before this stage existed
            voices::play_not_found();
            ipc::send(IpcEvent::Error {
                message: format!("Command not found: {}", text)
            });
        } else {
            match llm::LlmConfig::from_settings() {
                Ok(cfg) => {
                    // not_found.wav normally does NOT play here. it is a canned
                    // "no", and with no TTS in this stage that sound plus the
                    // GUI IS the whole reply - announcing failure and then
                    // quietly putting an answer on screen contradicts itself.
                    //
                    // UNLESS nobody is subscribed to the IPC broadcast: then the
                    // answer, and any error explaining its absence, is dropped
                    // by ipc::send and the turn produces no observable output at
                    // all. running with the GUI closed is a supported state
                    // (tray.rs open_settings checks the same thing), so in it
                    // the old audible reaction is still the whole reply.
                    if !ipc::has_clients() {
                        debug!("No IPC client attached; the LLM answer has nowhere to land.");
                        voices::play_not_found();
                    }
                    spawn_llm_turn(rt, cfg, text.to_string());
                }
                // switched on but unusable: no model name, or a remote endpoint
                // with llm_allow_remote off. old behaviour, plus the reason on
                // the wire - a half-configured LLM that is silent is unfixable.
                Err(e) => {
                    warn!("LLM turn skipped: {}", e);
                    voices::play_not_found();
                    ipc::send(IpcEvent::LlmAnswer {
                        request_id: String::new(),
                        prompt: text.to_string(),
                        answer: None,
                        model: String::new(),
                        elapsed_ms: 0,
                        error_code: Some(e.code().to_string()),
                        error: Some(e.to_string()),
                    });
                }
            }
        }
    }
    
    ipc::send(IpcEvent::Idle);
    false // no chain on error or not found
}


// ### LLM TURN (stage 1: text answer only - no tools, no streaming, no speech)

// generation of the newest LLM turn. the request is spawned and the utterance
// returns immediately, so two quick "no command found"s put two answers in
// flight; without this the slower one lands last and overwrites the newer
// question's answer in the GUI.
static LLM_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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
fn spawn_llm_turn(rt: &tokio::runtime::Runtime, cfg: llm::LlmConfig, prompt: String) {
    use std::sync::atomic::Ordering;

    let generation = LLM_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let request_id = generation.to_string();

    let handle = rt.spawn(async move {
        ipc::send(IpcEvent::LlmThinking {
            request_id: request_id.clone(),
            prompt: prompt.clone(),
        });

        let started = std::time::Instant::now();
        let result = llm::ask(&cfg, &prompt).await;
        let elapsed_ms = started.elapsed().as_millis() as u64;

        // a newer utterance started while this was in flight. its answer is the
        // one the user is waiting for; drop this one rather than race it.
        if LLM_GEN.load(Ordering::SeqCst) != generation {
            debug!("LLM answer for generation {} superseded, dropped", generation);
            return;
        }

        match result {
            Ok(a) => {
                // the text goes in the log too, {:?} so a multi-line answer
                // stays one line. with no GUI attached (see the has_clients
                // check at the hook) the log is the only place it lands.
                info!("LLM answered in {} ms ({} completion tokens, model {}): {:?}",
                      elapsed_ms, a.completion_tokens, a.model, a.text);
                ipc::send(IpcEvent::LlmAnswer {
                    request_id,
                    prompt,
                    answer: Some(a.text),
                    model: a.model,
                    elapsed_ms,
                    error_code: None,
                    error: None,
                });
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


pub fn close(code: i32) {
    info!("Closing application.");
    voices::play_goodbye();
    ipc::send(IpcEvent::Stopping);
    std::process::exit(code);
}