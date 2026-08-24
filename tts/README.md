# The speech sidecar

This is the voice. Jarvis asks a local language model for an answer and then has
to say it out loud in the assistant's own voice; neither the model nor the Rust
side can do that, so a small Python process does. It loads a zero-shot voice
cloning model, holds it on the GPU, and streams audio back over loopback HTTP
while the sentence is still being generated.

It is a separate process on purpose. The model is PyTorch, it takes seconds to
load and gigabytes of VRAM to hold, and restarting the assistant should not mean
paying for that again.

## What is here

| file | what it is |
|---|---|
| `sidecar_qwen.py` | the sidecar in use. Qwen3-TTS, streaming, with a web console |
| `sidecar.py` | the earlier CosyVoice sidecar. Same protocol, kept for comparison |
| `ttscommon.py` | the frame protocol and the take checks both sidecars share |
| `refslice.py` | cuts the reference sample, snapping to real pauses |
| `bake_pack_qwen.py` | bakes the fixed phrases into a voice pack, with retakes |
| `bake_pack.py` | the same for CosyVoice |
| `studio.py` | try lines and settings by hand before baking |
| `t0.py` | latency probe: cold, warm, streaming, whole-answer |
| `bench_llm.py` | language-model latency, first audible word rather than first token |
| `tts.ps1` | start, stop, check, unload |

Everything else this needs - the environments, the weights, the caches, the
generated audio - is deliberately not in the repository. See `.gitignore`.

## What is missing, and why

**The reference voice sample.** The clone is built from one recording, that
recording is yours, and it is not ours to publish. The scripts expect it at:

```
tts/xamples/jarvis_sample.wav
```

(`xamples` is a typo that became load-bearing - the scripts default to it.) Any
clean recording works. What matters is measured, not guessed:

- **8-10 seconds** of one speaker, no music, no second voice.
- **Cut at real pauses.** A cut through the middle of a word leaves the model
  finishing that word at the start of every line it generates - the phantom
  "есть"/"сесть" that took an evening to find. `refslice.py` snaps the cut
  points to sustained pauses, using a threshold relative to the surrounding
  speech: film dialogue sits around -26 dB and any absolute floor gets it wrong.

**The environment.** Python, PyTorch and the model code come to several
gigabytes and are not in git. Build it once:

```
uv venv venv-qwen --python 3.10
.\venv-qwen\Scripts\python.exe -m pip install torch==2.11.0+cu128 torchaudio==2.11.0 --index-url https://download.pytorch.org/whl/cu128
uv pip install --python .\venv-qwen\Scripts\python.exe -r requirements-qwen.txt
```

torch comes from PyTorch's own index because the CUDA builds are not on PyPI -
pick the one that matches your card. `requirements-qwen.txt` pins everything
else exactly as installed here. The older CosyVoice sidecar has its own
`requirements-cosyvoice.txt` and a separate `venv`.

**The model weights.** Nothing to do. The scripts point Hugging Face and
ModelScope at caches inside this folder, so the first run downloads what it
needs into `tts/hf` and `tts/models` and every run after that is offline.

## First run

```
.\tts.ps1
```

That starts the sidecar and opens the console. It comes up in a second or
two, because it does **not** take the model into video memory on the way: press
"Загрузить в память" in the console when you want it, or just ask for speech and
it loads itself. The first load fetches the weights if they are not there yet.

That is the default because the model is about 5 GB. Started while something
else held the card, the load did not finish inside three minutes and the
launcher gave up on a process that was alive and merely thrashing. Pass
`-Preload` for the old behaviour.

Put your reference recording in `xamples/` first, or it has no voice to clone.

## Running it

```
.\tts.ps1                 # start the Qwen sidecar and open its console
.\tts.ps1 -Status         # is it up, what is loaded, how much VRAM
.\tts.ps1 -Unload         # drop the model, keep the process
.\tts.ps1 -Reload         # load it again
.\tts.ps1 -Stop           # stop the process
.\tts.ps1 -Engine cosy    # the CosyVoice sidecar instead
.\tts.ps1 -Preload        # load the model at startup, the way it used to be
.\tts.ps1 -Foreground     # keep it attached to this console
.\tts.ps1 -ResetConfig    # forget the saved settings and start from defaults
```

It listens on `127.0.0.1:8772`. Jarvis itself will talk to nothing else - the
whole point is that speech is synthesised on this machine - and while `--host`
exists, moving off the loopback switches `/gpu/kill` off: a console that ends
processes is for the person at this keyboard, not for the network.

The per-process figures on the Видеопамять tab come from Windows' own
performance counters, the ones Task Manager draws, because `nvidia-smi` reports
no per-process memory at all under the WDDM driver - every row comes back
`[N/A]`. They do not add up to the card's total and are not meant to: `dwm`
holds the composition surfaces for every window on the desktop, so the same
memory is counted under `dwm` and again under the application that drew it. The
card's own total is read separately, from the driver.

`dwm`, `explorer`, `csrss` and the rest of the session cannot be ended from
there, whatever they are holding, and neither can the sidecar itself - to free
what IT holds, unload the model.

## The clips Jarvis plays by itself

Twenty-three of them, in `resources/sound/voices/jarvis-og-tts/ru/`: "да, сэр"
when it hears its name, "думаю над ответом" while an answer is on its way,
"выполнено" when a command ran. They are baked from the same reference the live
voice clones from, so **a new reference makes them the wrong voice** - they say
the same words in the old one until they are recorded again.

The Заготовки tab lists them with what each says, plays any of them, and
re-records one or all. What a clip says is now part of the pack: `voice.toml`
carries a `[lines.ru]` table, so the pack can tell you what `reply2.wav` is
supposed to be without opening a script in another folder. Jarvis ignores that
table - serde skips fields it does not know - and a test pins that it still
parses.

Each take is checked the way every spoken answer is, and a mangled one is
retaken. That is a weaker check than `bake_pack_qwen.py` does - the script also
transcribes each take with whisper and compares it to the line, six attempts
across four temperatures - but the script needs whisper, which is deliberately
not installed here, and in the console a person is listening with the play
button right beside the record button. The previous file is kept in
`jarvis-og-tts-previous/` before it is overwritten: the model never produces the
same take twice.

## Voices, and which model can do what

| family | how the voice is chosen | reference | instruct |
|---|---|---|---|
| `-Base` | cloned from the reference recording | required | usually read aloud instead of followed |
| `-CustomVoice` | one of nine built-in names | ignored | controls the manner |
| `-VoiceDesign` | described in words | ignored | required - it is the whole input |

The console follows the loaded model: the built-in voice list appears only for
CustomVoice, and the note under the instruction field changes to say whether it
will be followed or spoken. Language is a dropdown; `Auto` lets the model decide
from the text, which helps when an answer has English words in it.

| route | what it does |
|---|---|
| `/` | the console, in three tabs: **Синтез** to try a line, change sampling, switch model, voice or reference; **Заготовки** to see and re-record the clips Jarvis plays without asking the model; **Видеопамять** to see what is holding the card |
| `/speak` | synthesise. Length-prefixed frames, streamed |
| `/health` | up, which model, sample rate, which reference |
| `/models` | what can be loaded, and what each one is good for |
| `/model` | load a different one |
| `/pack` | the voice pack: every clip, what it says, how long it runs |
| `/pack/audio` | one clip, to listen to |
| `/pack/line` | change what a clip is supposed to say |
| `/pack/bake` | record one clip again in the voice loaded now |
| `/gpu` | the card, and every process holding video memory |
| `/gpu/kill` | end one of them |
| `/config` | the saved settings, read and write |
| `/reference` | the reference sample and how it is sliced |
| `/unload`, `/reload` | free the VRAM, take it back |

Settings written through the console are saved to `sidecar_qwen_config.json` and
**beat the command line** on the next start - so a model chosen in the console
stays chosen. `-ResetConfig` is the way out of that.

## Choosing a model

`/models` lists them with a note each. In short:

- **`Qwen3-TTS-12Hz-1.7B-Base`** - the default, and what ships. Best prosody
  and the steadiest from take to take. A few gigabytes.
- `Qwen3-TTS-12Hz-0.6B-Base` - about 1.5 GB, faster, noticeably rougher.
- The `-CustomVoice` and `-VoiceDesign` variants **ignore the reference sample
  entirely**. They are not for cloning and will not sound like your recording.

## Two things worth knowing before changing anything

**The first request after loading is slow** - 5701 ms against 590 ms once warm.
The sidecar therefore generates one throwaway line at startup, so the first
thing you actually ask for is not the one that pays for it.

**Markup is not spoken here.** CosyVoice understood `<strong>` and `[breath]`;
Qwen reads them out as words - "бред", "строк". Jarvis strips all of it before
sending, and there is a test pinning that. If you switch engines again, that is
the first thing to revisit.
