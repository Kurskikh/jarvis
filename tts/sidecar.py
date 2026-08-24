"""
Speech sidecar for Jarvis: text in, audio out, nothing else.

It knows nothing about the language model, the wake word or the command
engine. Jarvis decides what to say and how to cut it up; this process only
turns a piece of text into a piece of audio in the assistant's voice, and does
it fast enough to be worth waiting for.

Two things it does that a naive wrapper would not:

  - it defends against CosyVoice's failure modes. The model intermittently
    returns pure silence, a one-syllable stub, or trips its own vocoder with
    "kernel size can't be greater than actual input size". All three are
    sampling-dependent and all three are invisible unless you look, so every
    take is checked before it is handed back, and a bad one is taken again.

  - it holds the reference slice fixed. The voice here has to be the same
    voice as the pre-baked clips, or the assistant will answer its own canned
    "Думаю над ответом, сэр" in a subtly different one. Same file, same slice,
    same code (refslice.py), computed once at startup.

    python sidecar.py --reference "xamples/jarvis_sample.wav"
"""
import argparse
import io
import os
import struct
import sys
import threading
import time
from pathlib import Path

HERE = Path(__file__).parent
COSY = HERE / "CosyVoice"

os.environ.setdefault("HF_HOME", str(HERE / "hf"))
os.environ.setdefault("MODELSCOPE_CACHE", str(HERE / "modelscope"))

import gc

import numpy as np
import soundfile as sf
import torch
import torchaudio


# torchaudio 2.11 routes IO through TorchCodec, which wants system FFmpeg
def _load(path, *_a, **_kw):
    data, sr = sf.read(str(path), dtype="float32", always_2d=True)
    return torch.from_numpy(data.T.copy()), sr


def _save(path, tensor, sample_rate, *_a, **_kw):
    arr = tensor.detach().cpu().numpy()
    if arr.ndim == 2:
        arr = arr.T
    sf.write(str(path), arr, int(sample_rate))


torchaudio.load = _load
torchaudio.save = _save

sys.path.insert(0, str(COSY))
sys.path.insert(0, str(COSY / "third_party" / "Matcha-TTS"))
sys.path.insert(0, str(HERE))

from fastapi import FastAPI, Body
from fastapi.responses import StreamingResponse, JSONResponse

import refslice
from ttscommon import (FLAG_FELL_BACK, END, frame, wav_bytes,
                       bad_take, trim_head, syllables,
                       transcript_cache)

# ------------------------------------------------------------------ state
app = FastAPI()
CFG = None
_model = None
_lock = threading.Lock()      # one GPU, one generation at a time
_slice_path = None
_slice_secs = 0.0
_prompt_text = ""
_initial_hop = None      # CosyVoice's trained chunk size, captured before it drifts
_stats = {"requests": 0, "retakes": 0, "failures": 0, "fallbacks": 0}


def model():
    global _model, _initial_hop
    if _model is None:
        os.chdir(COSY)
        from cosyvoice.cli.cosyvoice import AutoModel
        t = time.time()
        _model = AutoModel(model_dir=str(CFG.model_dir), fp16=True)
        inner = getattr(_model, "model", None)
        if inner is not None:
            _initial_hop = getattr(inner, "token_hop_len", None)
        print(f"model loaded in {time.time()-t:.1f}s "
              f"(token_hop_len {_initial_hop})", flush=True)
    return _model


def unload():
    """
    Drop the model and hand the video memory back.

    Three models want this card at once - the language model in LM Studio, the
    other speech sidecar, and this one - and 16 GB does not stretch forever.
    Whichever is idle should be the one to let go. The next /speak reloads,
    paying the ten seconds again.
    """
    global _model, _initial_hop
    if _model is None:
        return False
    _model = None
    _initial_hop = None
    gc.collect()
    if torch.cuda.is_available():
        torch.cuda.empty_cache()
        torch.cuda.ipc_collect()
    print("model unloaded, video memory released", flush=True)
    return True


def vram():
    if not torch.cuda.is_available():
        return None
    free, total = torch.cuda.mem_get_info()
    # same shape as the Qwen sidecar: the launcher reads both, and two
    # spellings of one idea is how one of them goes stale
    mb = lambda b: round(b / 2**20)
    return {
        "card_total_mb": mb(total),
        "card_free_mb": mb(free),
        "card_used_mb": mb(total - free),
        "ours_live_mb": mb(torch.cuda.memory_allocated()),
        "ours_held_mb": mb(torch.cuda.memory_reserved()),
    }


def transcribe(path):
    cache = transcript_cache(path, HERE)
    if cache.exists():
        return cache.read_text(encoding="utf-8").strip()
    import whisper
    w = whisper.load_model("small")
    text = w.transcribe(str(path), language="ru")["text"].strip()
    del w
    if torch.cuda.is_available():
        torch.cuda.empty_cache()
    cache.write_text(text, encoding="utf-8")
    return text


def reset_hop(m):
    """
    Put CosyVoice's streaming chunk size back where it started.

    Its streaming loop grows token_hop_len as it goes ("increase
    token_hop_len incrementally to avoid duplicate inference") but grows the
    INSTANCE attribute and never restores it, so the value survives into the
    next request. The first stream after startup waits 25 tokens for its
    first frame; every one after that waits up to 100 - four times the delay,
    for the rest of the process's life.

    Measured here as 2604 ms on the first call and 3643 ms on a later one for
    the same 55-character line, which reads as noise until you find this.

    Not fixed by editing CosyVoice: this survives updating it.
    """
    inner = getattr(m, "model", None)
    if inner is not None and _initial_hop is not None:
        inner.token_hop_len = _initial_hop


def synth_once(text: str, stream: bool, speed: float = 1.0, instruct: str = ""):
    """
    One attempt; yields (audio, sr) pieces. Raises on vocoder failure.

    With an instruction, CosyVoice takes a different path. frontend_instruct2
    is frontend_zero_shot with two substitutions: the reference TRANSCRIPT is
    replaced by the instruction, and llm_prompt_speech_token is deleted, so
    the language half of the model no longer hears the reference at all. The
    speaker embedding and the flow prompt survive.

    In plain terms: the timbre stays, the MANNER does not. Without an
    instruction the assistant copies how the person in the sample speaks;
    with one, delivery comes from the words of the instruction instead. That
    is a real trade and not obviously a win, which is why it is off unless
    asked for.
    """
    m = model()
    if stream:
        reset_hop(m)
    if instruct.strip():
        # CosyVoice3 asserts on the marker in the instruct path too, where it
        # goes at the END of the instruction - that is the convention its own
        # examples use ("говори медленно<|endofprompt|>"). Without it the
        # request dies in a worker thread and surfaces as a vocoder error two
        # layers away, which is a genuinely confusing way to learn this.
        tagged_instruct = instruct.strip()
        if "<|endofprompt|>" not in tagged_instruct:
            tagged_instruct += "<|endofprompt|>"
        for out in m.inference_instruct2(text, tagged_instruct, str(_slice_path),
                                         stream=stream, speed=speed):
            yield out["tts_speech"].squeeze(0).cpu().numpy(), m.sample_rate
        return
    tagged = f"You are a helpful assistant.<|endofprompt|>{_prompt_text}"
    for out in m.inference_zero_shot(text, tagged, str(_slice_path),
                                     stream=stream, speed=speed):
        yield out["tts_speech"].squeeze(0).cpu().numpy(), m.sample_rate


def synth_checked(text: str, tries: int = 3, speed: float = 1.0, instruct: str = "",
                  keep_bad: bool = False):
    """
    One shot synthesis with the take checked and retaken if it is no good.

    Returns (audio, sr). Raises RuntimeError if every attempt failed - the
    caller stops the answer rather than skipping a sentence, because a
    silently missing sentence still sounds like a complete answer.
    """
    last = "no attempt"
    for attempt in range(1, tries + 1):
        try:
            pieces = [a for a, _ in synth_once(text, stream=False, speed=speed, instruct=instruct)]
            sr = model().sample_rate
            audio = np.concatenate(pieces) if pieces else np.zeros(0, dtype="float32")
        except RuntimeError as e:
            last = f"vocoder: {e}"
            _stats["retakes"] += 1
            print(f"  attempt {attempt}: {last}", flush=True)
            continue
        why = bad_take(audio, sr, text)
        if keep_bad and why is not None:
            print(f"  keeping a bad take for inspection: {why}", flush=True)
            return audio, sr
        if why is None:
            if attempt > 1:
                print(f"  attempt {attempt}: ok", flush=True)
            return audio, sr
        last = why
        _stats["retakes"] += 1
        print(f"  attempt {attempt}: {why}, retaking", flush=True)
    _stats["failures"] += 1
    raise RuntimeError(f"{tries} attempts failed, last: {last}")


# --------------------------------------------------------------- endpoints
@app.get("/health")
def health():
    return {
        "ok": _model is not None,
        "model": str(CFG.model_dir.name),
        "sample_rate": _model.sample_rate if _model else None,
        "reference": str(CFG.reference),
        "slice": {"start": CFG.start, "length": CFG.length,
                  "secs": round(_slice_secs, 2), "snap": CFG.snap},
        "prompt_text": _prompt_text,
        "token_hop_len": {
            "initial": _initial_hop,
            "now": getattr(getattr(_model, "model", None), "token_hop_len", None),
        },
        "stats": dict(_stats),
        "vram": vram(),
    }


@app.post("/unload")
def api_unload():
    """Hand the video memory back. The next /speak loads the model again."""
    with _lock:
        was = unload()
    return {"unloaded": was, "vram": vram()}


@app.post("/reload")
def api_reload():
    """Load it back without waiting for a question to pay the cost."""
    with _lock:
        t0 = time.time()
        model()
    return {"loaded": True, "took_secs": round(time.time() - t0, 1), "vram": vram()}


@app.post("/speak")
def speak(body: dict = Body(...)):
    text = (body.get("text") or "").strip()
    mode = (body.get("mode") or "sentence").lower()
    try:
        speed = float(body.get("speed", 1.0))
    except (TypeError, ValueError):
        speed = 1.0
    speed = min(2.0, max(0.5, speed))
    instruct = (body.get("instruct") or "").strip()
    if instruct and mode == "stream":
        # An instruction and streaming cannot be combined safely. The model
        # sometimes reads the instruction ALOUD instead of obeying it - seen
        # with Russian, English and Chinese alike - and the only defence is
        # checking the finished take and asking again. In streaming there is
        # no finished take to check: the frames are already on their way to
        # the speaker by the time anything is wrong.
        #
        # So an instruction forces one shot. It costs about a second of
        # latency and buys the ability to reject a take that is speaking the
        # instruction at the owner.
        print("instruct set - forcing one-shot mode so bad takes can be caught",
              flush=True)
        mode = "sentence"
    # diagnosis escape hatch: hand back the take even if it fails the
    # quality checks, so a bad one can be listened to instead of guessed at
    nocheck = bool(body.get("nocheck", False))
    if not text:
        return JSONResponse({"error": "text is empty"}, status_code=400)
    if mode not in ("sentence", "stream"):
        return JSONResponse({"error": f"unknown mode {mode!r}"}, status_code=400)

    def produce():
        with _lock:
            _stats["requests"] += 1
            t0 = time.time()
            total = 0.0
            try:
                if mode == "stream":
                    sent = 0
                    sr = model().sample_rate
                    try:
                        for audio, sr in synth_once(text, stream=True, speed=speed, instruct=instruct):
                            if audio.size == 0:
                                continue
                            if sent == 0:
                                audio = trim_head(audio, sr)
                            total += audio.size / sr
                            sent += 1
                            yield frame(wav_bytes(audio, sr))
                    except RuntimeError as e:
                        print(f"stream mode failed: {e}", flush=True)
                        sent = 0
                    if sent:
                        yield END
                        print(f"stream: {sent} frames, {total:.2f}s audio, "
                              f"{time.time()-t0:.2f}s", flush=True)
                        return
                    # the streaming path is the one that returns nothing at
                    # all. Fall back rather than answer with silence, but say
                    # so in the flags so an A/B comparison stays honest.
                    _stats["fallbacks"] += 1
                    print("stream produced no audio, falling back to one shot",
                          flush=True)
                    audio, sr = synth_checked(text, speed=speed, instruct=instruct)
                    yield frame(wav_bytes(trim_head(audio, sr), sr), FLAG_FELL_BACK)
                    yield END
                    return

                audio, sr = synth_checked(text, tries=1 if nocheck else 3, speed=speed,
                                          instruct=instruct, keep_bad=nocheck)
                audio = trim_head(audio, sr)
                total = audio.size / sr
                yield frame(wav_bytes(audio, sr))
                yield END
                took = time.time() - t0
                print(f"sentence: {total:.2f}s audio in {took:.2f}s "
                      f"(rtf {took/max(total,1e-6):.2f}) :: {text[:60]}", flush=True)
            except RuntimeError as e:
                # no frames, no END - a truncated body is how the client
                # learns this answer died, and it stops rather than
                # pretending the missing sentence was never there
                print(f"giving up on {text[:60]!r}: {e}", flush=True)

    return StreamingResponse(produce(), media_type="application/octet-stream")


# -------------------------------------------------------------------- main
def main():
    global CFG, _slice_path, _slice_secs, _prompt_text
    ap = argparse.ArgumentParser()
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=8771)
    ap.add_argument("--model-dir", default=str(HERE / "models" / "Fun-CosyVoice3-0.5B"))
    ap.add_argument("--reference", default=str(HERE / "xamples" / "jarvis_sample.wav"))
    # defaults are the settings the shipped voice pack was baked with; changing
    # them changes the voice, so they belong in one place
    ap.add_argument("--start", type=float, default=5.0)
    ap.add_argument("--length", type=float, default=8.0)
    ap.add_argument("--no-snap", dest="snap", action="store_false", default=True)
    a = ap.parse_args()
    a.model_dir = Path(a.model_dir)
    a.reference = Path(a.reference)
    CFG = a

    if not a.reference.exists():
        print(f"reference not found: {a.reference}", file=sys.stderr)
        return 2

    out = HERE / "studio_out" / "_sidecar_ref.wav"
    out.parent.mkdir(exist_ok=True)
    _slice_path, _slice_secs = refslice.slice_reference(
        a.reference, a.start, a.length, out, do_snap=a.snap)
    print(f"reference : {a.reference.name}  {a.start}+{a.length}s "
          f"-> {_slice_secs:.2f}s  snap={a.snap}")
    _prompt_text = transcribe(_slice_path)
    if _prompt_text and _prompt_text[-1] not in ".!?…":
        _prompt_text += "."
    print(f"transcript: {_prompt_text}")

    model()                      # warm before serving, so the first ask is not the slow one
    os.chdir(HERE)
    print(f"http://{a.host}:{a.port}", flush=True)

    import uvicorn
    uvicorn.run(app, host=a.host, port=a.port, log_level="warning")
    return 0


if __name__ == "__main__":
    sys.exit(main())
