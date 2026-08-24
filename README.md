# JARVIS Voice Assistant

![We are NOT limited by the technology of our time!](poster.jpg)

An offline voice assistant for Windows: it listens for a wake word, runs the
commands it knows, and asks a local language model about everything else — then
says the answer out loud in its own cloned voice.

Nothing leaves the machine. Speech recognition, intent matching, the language
model and speech synthesis all run locally, and the one setting that could point
the assistant at a remote endpoint is off by default and refused unless it is
turned on deliberately.

This is a fork of [Priler/jarvis](https://github.com/Priler/jarvis) by Абрахам
Тугалов, and it keeps that project's licence. What follows is what this fork
does today; **[Where this fork differs](#where-this-fork-differs)** lists what
was added.

Backend: 🦀 **[Rust](https://www.rust-lang.org/)** with ❤️ **[Tauri](https://tauri.app/)**.
Frontend: ⚡️ **[Vite](https://vitejs.dev/)** + 🛠️ **[Svelte](https://svelte.dev/)**.

## How a turn works

1. **Wake word.** Everything is buffered but nothing is acted on until the
   assistant hears its name.
2. **Command or question.** What follows is matched against the command packs by
   an intent classifier, with a fuzzy fallback.
3. **A command runs**, or — if nothing matches — the utterance goes to a local
   language model.
4. **The answer is spoken** in the assistant's voice, streamed so it starts
   talking before the whole sentence has been synthesised.
5. **The microphone stays open** for a moment afterwards, so a follow-up needs
   no wake word.

## What it can do

**Commands.** Twelve packs ship with it: browser, calculator, counter, Steam,
volume, weather, windows, Yandex Music, plus the assistant's own (stop,
terminate, and a slot-extraction demo). A pack is a TOML file with phrases and
what to run — AutoHotkey, a shell command, a Lua script — and the intent
classifier retrains itself whenever the packs change.

**Yandex Music** is driven through the Windows media session rather than media
keys, so switching a track does not pause a browser tab and does not pull the
player in front of a game. Starting playback from cold is the one case that
needs the window, because the media session does not exist until the first note.

**Answers from a language model.** Any OpenAI-compatible local endpoint — LM
Studio, ollama — configured in settings, with the model picked from a list the
server itself reports. Reasoning models can have their thinking turned off
through a prompt directive, since a spoken answer that arrives ten seconds late
is not an answer.

**Its own voice.** Answers are spoken through a speech sidecar that clones a
voice from one reference recording. See [`tts/`](tts/).

## Under the hood

- **Speech-to-text** — [Vosk](https://github.com/alphacep/vosk-api) via
  [vosk-rs](https://github.com/Bear-03/vosk-rs).
- **Wake word** — [Rustpotter](https://github.com/GiviMAD/rustpotter),
  [Picovoice Porcupine](https://github.com/Picovoice/porcupine) (needs a key),
  or Vosk with a restricted grammar.
- **Intent** — sentence embeddings, multilingual or English-only, over the
  phrases in the command packs.
- **Text-to-speech** — a local sidecar running
  [Qwen3-TTS](https://huggingface.co/Qwen) for zero-shot cloning, with a
  [CosyVoice](https://github.com/FunAudioLLM/CosyVoice) sidecar kept alongside
  for comparison. Fixed phrases are pre-baked into a voice pack; answers are
  synthesised as they arrive.
- **Audio** — [kira](https://github.com/tesselode/kira), with chunks stitched on
  a clock so streamed speech has no click between them.

Interface, wake word and recognition are available in **Russian**, **English**
and **Ukrainian**.

## Where this fork differs

- Answers from a local language model when no command matches, spoken aloud as
  they stream in.
- A speech sidecar with voice cloning, its own web console, and its source in
  [`tts/`](tts/).
- Yandex Music control through the Windows media session.
- Settings for all of the above: endpoint, model list, timeouts, reasoning,
  system prompt, speaking, sidecar, follow-up window.
- A wake word that cannot turn into a question. The detector and the command
  recogniser read the same audio, and the recogniser used to render the wake
  word as a word of its own — "баржа", "карлос" — which then went to the
  language model as something the user never asked.
- Various fixes underneath: the recorder's frame size, the detector's defaults,
  and the assistant no longer transcribing its own voice as the next command.

## Building

You need Rust and Node.js. Install the dependencies, then:

```
cargo tauri build      # release
cargo tauri dev        # development
```

Platform libraries for [PvRecorder](https://github.com/Picovoice/pvrecorder) and
[Vosk](https://github.com/alphacep/vosk-api) are included under `lib/`.

The speech sidecar is a separate Python process and is not built by the above —
see [`tts/README.md`](tts/README.md). Without it the assistant still runs; it
just answers in text instead of speaking.

## Python version?

The project was originally written in Python. This is the Rust one.

## License

[Attribution-NonCommercial-ShareAlike 4.0 International](https://creativecommons.org/licenses/by-nc-sa/4.0/),
inherited from the upstream project. Attribution and the same licence on
derivatives are conditions of use, not courtesies. See LICENSE.txt.

The reference voice recording used to clone the assistant's voice is **not** in
this repository and is not covered by the above — it is a personal recording.
`tts/README.md` explains what to supply in its place.
