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

That starts the sidecar, fetches the voice model if it is not there yet,
generates one throwaway line to warm it up, and opens the console. Put your
reference recording in `xamples/` first, or it has no voice to clone.

## Running it

```
.\tts.ps1                 # start the Qwen sidecar and open its console
.\tts.ps1 -Status         # is it up, what is loaded, how much VRAM
.\tts.ps1 -Unload         # drop the model, keep the process
.\tts.ps1 -Reload         # load it again
.\tts.ps1 -Stop           # stop the process
.\tts.ps1 -Engine cosy    # the CosyVoice sidecar instead
.\tts.ps1 -Foreground     # keep it attached to this console
.\tts.ps1 -ResetConfig    # forget the saved settings and start from defaults
```

It listens on `127.0.0.1:8772`. Loopback only, and there is no setting to widen
that: the whole point is that speech is synthesised on this machine.

| route | what it does |
|---|---|
| `/` | the console: try a line, change sampling, switch model or reference |
| `/speak` | synthesise. Length-prefixed frames, streamed |
| `/health` | up, which model, sample rate, which reference |
| `/models` | what can be loaded, and what each one is good for |
| `/model` | load a different one |
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
