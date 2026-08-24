# Speaking language model answers in the assistant's voice

Status: design agreed, implementation in progress
Date: 2026-08-24

## The problem

The assistant answers questions from a local language model, but the answer
only ever reaches the log and the window. It is never spoken. Canned reactions
are pre-baked clips played from disk; a free-form answer cannot be, because
nobody knows it in advance.

So an answer has to be synthesised while the user waits, in the same voice as
the baked clips, without the wait being obvious.

## What was measured

Everything below was measured on this machine (5070 Ti, 16 GB) against the
real components, not estimated. The numbers drove every decision and several
of them overturned an earlier guess.

**Language model** (gemma-4-e4b-it-heretic via LM Studio, `bench_llm.py`):

| prompt | streamed, first sentence | one shot, whole answer |
|--------|--------------------------|------------------------|
| medium | 461 ms                   | 894 ms                 |
| long   | 562 ms                   | 600 ms                 |

Streaming the model buys 40-430 ms. Synthesis needs ~1900 ms afterwards either
way, and the "Думаю над ответом, сэр" clip covers 1.6-2.8 s of that, so the
saving is spent inside a clip that is still playing. **Server-sent events are
not implemented.** `llm::ask` stays one shot.

**Synthesis** (CosyVoice 3 through the sidecar, `bench_frames.py`,
`bench_repeat.py`). The cost of a single non-streaming call fits
`1550 ms + 34 ms per character` across five lengths from 7 to 187 characters.
The fixed 1550 ms is per call, which is what makes fine-grained chunking a
loss rather than a win: every chunk pays it again.

Streaming versus one shot, same lines, five runs each, after the fixes below:

| mode     | first speech    | spread across runs |
|----------|-----------------|--------------------|
| stream   | 1848 - 2396 ms  | 542 ms             |
| sentence | 3162 - 7422 ms  | 2619 ms            |

Streaming wins on latency and wins far more decisively on predictability.
Frame arrivals were simulated against a playback clock: no stalls at any
length tested. Frame one carries 0.5-1.0 s of leading silence, later frames
carry none at either edge, so the seams are naturally gapless and only the
very start is wasted. The sidecar trims that head.

## Two defects found and fixed in the sidecar

**CosyVoice never resets its streaming chunk size.** `token_hop_len` starts at
25, the streaming loop multiplies it by `stream_scale_factor` up to a ceiling
of 100, and it mutates the instance attribute. Nothing puts it back. The first
streamed request of a process waits 25 tokens for its first frame; every
request after it waits up to 100. Measured as 2604 ms on an early call and
3643 ms on a later one for the same line - indistinguishable from noise unless
you look for it. The sidecar restores the captured initial value before each
streaming call and reports both values from `/health`, so a future drift is
visible rather than mysterious. Fixed by restoring rather than by editing
CosyVoice, so upgrading the model does not undo it.

**CosyVoice intermittently returns nothing usable.** Pure silence, a
one-syllable stub, or a vocoder failure ("kernel size can't be greater than
actual input size"). All three are sampling-dependent. Baking the voice pack
hit silence twice in a row on one line and needed five takes on another. The
sidecar checks every take - peak level, and duration against a floor derived
from the character count - and takes it again up to three times. A whisper
pass would be more thorough but costs more than the synthesis it guards.

## Architecture

```
jarvis-app                      sidecar (Python, 127.0.0.1:8771)
  llm::ask  (one shot)
      |
      v
  speech::say(text) ---- POST /speak ----> CosyVoice 3, warm
      |                                       |
      |   <---- length-prefixed frames -------+
      v
  playback queue (kira clock)
```

The sidecar knows nothing about the language model, the wake word or the
command engine: text in, audio out. Jarvis decides what to say; the sidecar
only says it.

**No text chunker.** An earlier draft had one in Rust, cutting Russian into
clauses with a ramp - short first chunk, longer afterwards. The measurements
killed it: CosyVoice's own streaming emits frames sooner than any chunking
scheme can, because chunking pays the 1550 ms call overhead per piece. The
whole answer goes in one call. A module, its Russian abbreviation and decimal
handling, and its tests all disappeared with it.

**No `/cancel`.** Inference is not interruptible mid-call. Cancelling means
clearing the queue, issuing no further calls and discarding the response in
flight. That wastes about two seconds of GPU and removes an endpoint, a
request registry and a class of races.

### Frame protocol

The response body is a sequence of frames, so one reader serves both modes:

```
u32 le   payload length (0 ends the stream)
u32 le   flags: bit0 final, bit1 fell back from stream to one shot
         payload: WAV, 24 kHz mono
```

A body that ends without a zero-length frame means the answer died. The client
stops rather than treating a partial answer as complete.

### Gapless playback

Verified against kira 0.11: `StaticSoundData::from_cursor` plays from memory,
`AudioManager::add_clock` plus `StartTime::ClockTime` schedules a sound to an
absolute tick. A clock at 1000 ticks per second puts each frame at the tick
where the previous one ends, so seams fall inside a millisecond. Chained
`StartTime::Delayed` is not used: it accumulates error frame by frame.

### Voice coherence

The sidecar's reference, slice and slicing code are the same ones the shipped
pack was baked from (`jarvis_sample.wav`, 5 s in, 8 s long, snapped to pauses,
resolving to 9.94 s). If they diverge, the assistant answers its own canned
"Думаю над ответом, сэр" in a subtly different voice. The slicing lives in
`refslice.py`, shared by the sidecar and the studio so the two cannot drift.

### Failure behaviour

- Sidecar unreachable: the assistant works exactly as before. The answer still
  reaches the window and the log. Speech is a bonus, never a dependency.
- A take fails three times: the answer stops. It is not skipped silently - a
  missing sentence still sounds like a complete answer and misleads.
- Sidecar dies mid-answer: the body truncates, playback stops, the supervisor
  restarts the process.
- Spawned children are killed on exit; a sidecar that was already running when
  Jarvis started is left alone.

## What the first real run changed

Three things only showed up once a person asked a question out loud.

**The stream ran dry between chunks.** Measured by the stall detector built
into the playback queue: 950 ms of silence before the second chunk, 220 ms
before the third. The cause is the hop reset above interacting with
CosyVoice's own ramp - frame one is 25 tokens, about half a second of audio
after its head is trimmed, while frame two is 50 tokens and takes over a
second to make. Playing frame one the instant it lands therefore guarantees
running dry. Fixed with a 900 ms jitter buffer on the first chunk only: the
delay is paid once, at the start, where the "thinking" clip is still covering,
and it buys an answer that does not stumble in the middle - the half nothing
can mask. Later stalls are now logged at warn, not debug: if they reappear the
pre-roll is too short for the machine.

**The model answered without punctuation.** "У меня всё хорошо спасибо что
спросили Я готов помочь вам с любой задачей" - not one mark in the line.
Punctuation is what CosyVoice reads rhythm and phrase melody off, so the whole
answer came out as one flat run. The same model punctuates correctly when
asked to, so `LLM_SPEECH_STYLE_DIRECTIVE` is appended to the system prompt
whenever speaking is on. It is appended rather than baked into
`DEFAULT_LLM_SYSTEM_PROMPT` because that prompt belongs to the owner, and
someone who rewrites it should not silently lose this.

**The language model was cold.** 9931 ms for 24 tokens, against 894 ms
measured warm - LM Studio had unloaded it. Nothing to fix in this codebase,
but it means the first question after a pause is always slow, and no amount of
synthesis tuning changes that.

## Expression markup

CosyVoice 3 accepts these, from its tokenizer's `additional_special_tokens`:

| markup | effect |
|--------|--------|
| `.` `,` `?` `!` `…` | the main lever: pauses and phrase melody |
| `<strong>…</strong>` | stress the wrapped words |
| `[breath]`, `[quick_breath]` | a breath, i.e. a natural pause |
| `[sigh]`, `[laughter]`, `<laughter>…</laughter>` | sigh, a laugh, laughing while speaking |
| `[cough]`, `[lipsmack]`, `[mn]`, `[noise]` | other human noises |
| `inference_instruct2(text, instruction, …)` | plain-language instruction for emotion, speed, volume |

Two limits worth knowing. Phoneme inpainting exists only for English (CMU) and
Chinese (pinyin) - **there is no Russian phoneme set**, so pronunciation cannot
be corrected word by word. And the model card says number and symbol
normalisation is built in, which makes the missing `wetext` far less serious
than it looked.

The prompt directive permits only `<strong>` and `[breath]`; the rest are
listed in `speech::strip_markup` so that a model reaching for one anyway does
not print it in the window. Display and log get the stripped text, the sidecar
gets the text as written. `inference_instruct2` is not wired up: it takes a
different frontend path that may not preserve the cloned voice, and that needs
measuring before it is offered.

## Follow-up listening

Saying the wake word before every sentence is what makes an assistant feel
like a command line, so after a turn the microphone stays open for
`follow_up_secs` (default 8, 0 disables).

The subtlety is when the window opens. A language model turn answers seconds
after the command returns and then reads the answer out for ten more, so a
timer started at the command would be over before the assistant stopped
talking. The countdown is therefore held back while `llm_busy()` - a turn in
flight, or audio still playing - and only starts once there is silence to
speak into. The in-flight count is a counter rather than a flag because
superseding spawns the replacement before aborting the original.

## Settings

| key | default | meaning |
|-----|---------|---------|
| `llm_speak` | on | speak answers at all |
| `llm_tts_url` | `http://127.0.0.1:8771` | sidecar endpoint |
| `llm_tts_mode` | `stream` | `stream` or `sentence`, for comparison |
| `llm_tts_python` | empty | interpreter to spawn the sidecar with; empty means connect only |
| `llm_tts_script` | empty | path to sidecar.py, needed only alongside the interpreter |
| `follow_up_secs` | 8 | seconds to keep listening after the assistant stops speaking; 0 disables |

## Rejected

**Pipecat.** A good fit for the problem class, a poor one for this codebase.
Its pipeline runs microphone to speaker, and all of that already exists here
in Rust: PvRecorder, Rustpotter, Vosk, kira, the command engine, the editor,
the tray. Adopting it means either re-platforming a working voice loop into
Python or importing a real-time agent framework as a wrapper around "text in,
PCM out". There is no official CosyVoice service in it either - the one public
attempt (pipecat-ai/pipecat#4558) reported silences, unpredictable latency and
zero audio chunks, was closed without a fix, and the author moved to
ElevenLabs. Those are the same three failures measured here, so it offers no
head start on the hard part. Its strongest feature, interruption handling, is
unused here: stopping is a button.

Worth revisiting if the goal becomes full duplex conversation with barge-in,
freely swappable speech services, or reaching the assistant from a phone.

## Open

- Barge-in. The microphone is deaf while the assistant speaks
  (`audio::is_speaking`), so stopping is a button in the window and the tray.
  Voice interruption needs a selective gate: listen for the wake word only, at
  a raised threshold, so the assistant does not trigger on itself.
- The window's stop button appears with the answer, not with the speech.
  Nothing tells the window that speaking has begun or ended, so the button
  outlives the audio and can be pressed when there is nothing to stop, which
  does nothing. Tying it to the real state needs a pair of IPC events; the
  tray item has the same limitation and neither is worth the plumbing until
  the rest has been lived with.
- Answer length. Answers ran 102-350 characters, which is 6-20 seconds of
  speech. A cap in the system prompt is likely wanted.
- `wetext` normalisation is unavailable (ModelScope 403), so numbers may be
  read wrong.
