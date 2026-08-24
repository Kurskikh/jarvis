"""
T0 - the go/no-go measurement, before a single line of Rust.

Answers three questions:
  1. does CosyVoice 3 run at all on this Blackwell card
  2. how long does one Russian sentence take, warm, and what is the RTF
  3. what does the existing Jarvis voice sound like cloned from ~6 seconds

Writes its output wavs next to itself so they can simply be played.
"""
import os
import sys
import time
import wave
from pathlib import Path

HERE = Path(__file__).parent
COSY = HERE / "CosyVoice"

# keep the multi-GB weights off C:, which is at 95%
os.environ.setdefault("HF_HOME", str(HERE / "hf"))
os.environ.setdefault("MODELSCOPE_CACHE", str(HERE / "modelscope"))

# torchaudio 2.11 dropped its own backends and routes load/save through
# TorchCodec, whose DLLs need FFmpeg present on the system. soundfile is already
# a CosyVoice dependency and ships libsndfile with it, so route through that
# instead of adding a system-wide FFmpeg install to the sidecar's requirements.
import numpy as _np
import soundfile as _sf
import torch as _torch
import torchaudio as _ta


def _load(path, *_a, **_kw):
    data, sr = _sf.read(str(path), dtype="float32", always_2d=True)
    return _torch.from_numpy(data.T.copy()), sr


def _save(path, tensor, sample_rate, *_a, **_kw):
    arr = tensor.detach().cpu().numpy()
    if arr.ndim == 2:
        arr = arr.T
    _sf.write(str(path), arr, int(sample_rate))


_ta.load = _load
_ta.save = _save

sys.path.insert(0, str(COSY))
sys.path.insert(0, str(COSY / "third_party" / "Matcha-TTS"))

MODEL_DIR = HERE / "models" / "Fun-CosyVoice3-0.5B"
# the reference is YOUR file and never lives in the repo. pass it as the
# first argument; the transcript is cached per clip so whisper runs once.
#   python t0.py "D:/samples/jarvis_ru.wav"
if len(sys.argv) > 1:
    REFERENCE = Path(sys.argv[1])
else:
    raise SystemExit("usage: python t0.py <reference.wav>  "
                     "(6-30s, one speaker, no music)")
PHRASE = "Думаю над ответом, сэр."
# CosyVoice warns when the target is much shorter than the reference; a normal
# sentence shows what an actual answer will sound like
LONG = ("Все системы работают в штатном режиме, сэр. "
        "Свободно десять гигабайт видеопамяти, процессор загружен на двенадцать процентов.")


def secs(path):
    with wave.open(str(path)) as w:
        return w.getnframes() / float(w.getframerate())


def step(msg):
    print(f"\n>>> {msg}", flush=True)


# ---------------------------------------------------------------- weights
step("model weights")
if not (MODEL_DIR / "cosyvoice.yaml").exists() and not any(MODEL_DIR.glob("*.pt")):
    from huggingface_hub import snapshot_download
    t = time.time()
    snapshot_download("FunAudioLLM/Fun-CosyVoice3-0.5B-2512", local_dir=str(MODEL_DIR))
    print(f"    downloaded in {time.time()-t:.0f}s")
else:
    print("    already present")

# ------------------------------------------------------- reference transcript
step("reference clip")
print(f"    {REFERENCE.name}  {secs(REFERENCE):.2f}s")

transcript_file = HERE / f"reference_text_{REFERENCE.stem}.txt"
if transcript_file.exists():
    prompt_text = transcript_file.read_text(encoding="utf-8").strip()
    print(f"    transcript (cached): {prompt_text!r}")
else:
    # CosyVoice needs to know what the reference SAYS, not just how it sounds
    import whisper
    t = time.time()
    w = whisper.load_model("small")
    prompt_text = w.transcribe(str(REFERENCE), language="ru")["text"].strip()
    del w
    transcript_file.write_text(prompt_text, encoding="utf-8")
    print(f"    transcribed in {time.time()-t:.0f}s: {prompt_text!r}")

# ---------------------------------------------------------------- load model
step("loading CosyVoice")
os.chdir(COSY)  # the configs reference relative paths
import torch
import torchaudio
from cosyvoice.cli.cosyvoice import AutoModel

t = time.time()
cosyvoice = AutoModel(model_dir=str(MODEL_DIR), fp16=True)
load_s = time.time() - t
print(f"    loaded in {load_s:.1f}s, sample rate {cosyvoice.sample_rate}")
print(f"    VRAM allocated: {torch.cuda.memory_allocated()/2**30:.2f} GiB, "
      f"reserved {torch.cuda.memory_reserved()/2**30:.2f} GiB")


def synth(tag, stream, text=None):
    """one synthesis, timed from the call to the first chunk and to the last"""
    text = text or PHRASE
    t0 = time.time()
    first = None
    chunks = []
    # CosyVoice3 asserts on the <|endofprompt|> marker: prompt_text is a system
    # instruction plus the reference transcript, not the transcript alone
    tagged_prompt = f"You are a helpful assistant.<|endofprompt|>{prompt_text}"
    for out in cosyvoice.inference_zero_shot(text, tagged_prompt, str(REFERENCE), stream=stream):
        if first is None:
            first = time.time() - t0
        chunks.append(out["tts_speech"])
    total = time.time() - t0

    audio = torch.cat(chunks, dim=1)
    out_path = HERE / f"t0_{REFERENCE.stem}_{tag}.wav"
    torchaudio.save(str(out_path), audio, cosyvoice.sample_rate)
    dur = audio.shape[1] / cosyvoice.sample_rate

    print(f"    [{tag}] first chunk {first*1000:7.0f} ms | total {total*1000:7.0f} ms | "
          f"audio {dur:.2f}s | RTF {total/dur:.2f} | -> {out_path.name}")
    return total, dur


# a cold first call includes lazy CUDA graph/kernel setup; the number that
# matters for the product is the warm one
step("synthesis - cold")
synth("cold", stream=False)

step("synthesis - warm, non-streaming")
synth("warm", stream=False)

step("synthesis - warm, streaming (this is the number the design rides on)")
synth("stream", stream=True)

step("synthesis - warm, a full sentence")
synth("sentence", stream=False, text=LONG)

step("synthesis - streaming, a full sentence")
synth("sentence_stream", stream=True, text=LONG)

step("done")
print(f"    listen to: {HERE / 't0_warm.wav'}")
print(f"               {HERE / 't0_stream.wav'}")
