"""
The same speech sidecar, running Qwen3-TTS instead of CosyVoice.

Why a second process rather than a second option inside the first: CosyVoice
pins transformers 4.51.3 and qwen-tts wants 4.57.3. One of them would lose.
Splitting the environments costs a directory and a port; the frame protocol
was already engine-agnostic, so Jarvis switches between them by changing one
URL in its settings and needs no code change at all.

Everything that is not synthesis - the wire format, the quality checks, the
reference slicing - is shared with the CosyVoice sidecar (ttscommon.py,
refslice.py) so the two cannot drift.

    python sidecar_qwen.py --reference "I:/jarvis-tts/xamples/jarvis_sample.wav"
"""
import argparse
import json
import os
import sys
import threading
import time
from pathlib import Path

HERE = Path(__file__).parent
os.environ.setdefault("HF_HOME", str(HERE / "hf"))

import gc

import numpy as np
import soundfile as sf
import torch

sys.path.insert(0, str(HERE))

from fastapi import FastAPI, Body
from fastapi.responses import (StreamingResponse, JSONResponse, HTMLResponse,
                               Response)

import refslice
from ttscommon import (FLAG_FELL_BACK, END, frame, wav_bytes,
                       bad_take, trim_head, transcript_cache)

app = FastAPI()
CFG = None
_model = None
_lock = threading.Lock()      # one GPU, one generation at a time
_slice_path = None
_slice_secs = 0.0
_prompt_text = ""
_sample_rate = None
_stats = {"requests": 0, "retakes": 0, "failures": 0}


def model():
    global _model
    if _model is None:
        # faster-qwen3-tts rather than the plain qwen-tts package. Two reasons,
        # both verified against the code and not the README: it exposes
        # generate_voice_clone_streaming, which the plain package does not have
        # at all ("currently only simulates streaming text input ... rather
        # than enabling true streaming input or streaming generation"), and it
        # defaults to sdpa attention, so nothing has to build flash-attn on
        # Windows.
        #
        # Streaming is the whole game here. Without it latency grows with the
        # length of the answer - measured 2.2 s for a short line and 9.6 s for
        # a hundred characters - and no "thinking" clip covers that.
        from faster_qwen3_tts import FasterQwen3TTS
        t0 = time.time()
        _model = FasterQwen3TTS.from_pretrained(
            CFG.model_id,
            device="cuda" if torch.cuda.is_available() else "cpu",
            dtype=torch.bfloat16,
            attn_implementation="sdpa",
        )
        try:
            _model.warmup()
        except Exception as e:
            # not fatal: it only costs the first request some extra latency
            print(f"warmup skipped: {e}", flush=True)
        print(f"model loaded in {time.time()-t0:.1f}s", flush=True)
    return _model


def unload():
    """
    Drop the model and hand the video memory back.

    Three models want this card at once - the language model in LM Studio, the
    other speech sidecar, and this one - and 16 GB does not stretch forever.
    Whichever is idle should be the one to let go.
    """
    global _model
    if _model is None:
        return False
    _model = None
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
    return {"free_mb": round(free / 2**20), "total_mb": round(total / 2**20),
            "used_by_us_mb": round(torch.cuda.memory_allocated() / 2**20)}


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


# The model's sampling controls, with the package's own values as the floor.
#
# These are per-CALL parameters, not model state - nothing about the loaded
# weights changes when they move. But Jarvis asks for speech without naming
# any of them, so whatever is stored here is what a real answer gets. That is
# the whole reason they are saved rather than only accepted per request:
# tuning them in the console has to reach the assistant, not just the console.
KNOB_DEFAULTS = {"temperature": 0.9, "top_k": 50, "top_p": 1.0,
                 "repetition_penalty": 1.05, "max_new_tokens": 2048}

CONFIG_PATH = HERE / "sidecar_qwen_config.json"

# The Qwen3-TTS family, and which of it is any use here.
#
# Only the -Base variants clone a voice from a reference. CustomVoice ships
# built-in speakers and VoiceDesign builds one from a description; both ignore
# the reference entirely, so picking one would quietly replace Jarvis with
# somebody else. They are listed so the console can say that out loud rather
# than leaving it to be discovered.
KNOWN_MODELS = [
    {"id": "Qwen/Qwen3-TTS-12Hz-0.6B-Base", "clones": True,
     "note": "клонирование, ~1.5 ГБ, самая быстрая"},
    {"id": "Qwen/Qwen3-TTS-12Hz-1.7B-Base", "clones": True,
     "note": "клонирование, ~3.5 ГБ, лучше просодия и устойчивость"},
    {"id": "Qwen/Qwen3-TTS-12Hz-0.6B-CustomVoice", "clones": False,
     "note": "встроенные голоса, образец игнорируется"},
    {"id": "Qwen/Qwen3-TTS-12Hz-1.7B-CustomVoice", "clones": False,
     "note": "встроенные голоса, образец игнорируется"},
    {"id": "Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign", "clones": False,
     "note": "голос по описанию, образец игнорируется"},
]

# live defaults; loaded from disk at startup, replaced by /config
DEFAULTS = dict(KNOB_DEFAULTS)


# the reference slice as last applied, or None to keep what the command line said
SAVED_REFERENCE = None


def load_config():
    """
    Defaults from disk, ignoring anything unknown or unparseable.

    What was set in the console wins over the command line, always. The
    alternative - command line wins - means every restart silently undoes an
    afternoon of listening, and the flag that caused it is in a shortcut
    nobody reads. --reset-config is there for when the file is the problem.
    """
    global DEFAULTS, SAVED_REFERENCE
    DEFAULTS = dict(KNOB_DEFAULTS)
    SAVED_REFERENCE = None
    if not CONFIG_PATH.exists():
        return
    try:
        saved = json.loads(CONFIG_PATH.read_text(encoding="utf-8"))
    except (OSError, ValueError) as e:
        print(f"ignoring unreadable {CONFIG_PATH.name}: {e}", flush=True)
        return
    ref = saved.get("reference")
    if isinstance(ref, dict) and ref.get("path"):
        SAVED_REFERENCE = ref
    if saved.get("model_id"):
        DEFAULTS["model_id"] = str(saved["model_id"])
    for k, default in KNOB_DEFAULTS.items():
        if k in saved:
            try:
                DEFAULTS[k] = int(saved[k]) if isinstance(default, int) else float(saved[k])
            except (TypeError, ValueError):
                pass
    if "chunk_size" in saved:
        try:
            DEFAULTS["chunk_size"] = max(1, int(saved["chunk_size"]))
        except (TypeError, ValueError):
            pass
    print(f"defaults from {CONFIG_PATH.name}: "
          + ", ".join(f"{k}={v}" for k, v in DEFAULTS.items()), flush=True)
    if SAVED_REFERENCE:
        print(f"reference from {CONFIG_PATH.name}: {SAVED_REFERENCE['path']} "
              f"{SAVED_REFERENCE.get('start')}+{SAVED_REFERENCE.get('length')}s",
              flush=True)


def save_config():
    """
    Write both halves at once.

    The sampling values and the reference live in one file because they are
    one thing to a person: how the assistant sounds. Saving them separately
    invites the state where half of a tuning session survived a restart.
    """
    body = dict(DEFAULTS)
    if CFG is not None:
        body["reference"] = {"path": str(CFG.reference), "start": CFG.start,
                             "length": CFG.length, "snap": CFG.snap}
        body["model_id"] = CFG.model_id
    CONFIG_PATH.write_text(json.dumps(body, indent=2), encoding="utf-8")


def knobs_from(body: dict) -> dict:
    # DEFAULTS also carries model_id and chunk_size; only the sampling values
    # belong in a generate call
    """request values where given, saved defaults otherwise"""
    out = {}
    for k, default in KNOB_DEFAULTS.items():
        v = body.get(k, DEFAULTS.get(k, default))
        try:
            out[k] = int(v) if isinstance(default, int) else float(v)
        except (TypeError, ValueError):
            out[k] = DEFAULTS.get(k, default)
    return out


def synth_once(text: str, instruct: str = "", **knobs):
    """one attempt, whole answer at once; returns (audio, sample_rate)"""
    global _sample_rate
    m = model()
    wavs, sr = m.generate_voice_clone(
        text=text, language=CFG.language,
        ref_audio=str(_slice_path), ref_text=_prompt_text,
        instruct=instruct or None, **knobs,
    )
    _sample_rate = sr
    audio = wavs[0] if isinstance(wavs, (list, tuple)) else wavs
    if hasattr(audio, "detach"):
        audio = audio.detach().cpu().numpy()
    return np.asarray(audio, dtype="float32").squeeze(), sr


def synth_stream(text: str, instruct: str = "", chunk_size: int = None, **knobs):
    """yields (audio_chunk, sample_rate) as the model produces them"""
    global _sample_rate
    m = model()
    for chunk, sr, _timing in m.generate_voice_clone_streaming(
            text=text, language=CFG.language,
            ref_audio=str(_slice_path), ref_text=_prompt_text,
            chunk_size=chunk_size or DEFAULTS.get("chunk_size", CFG.chunk_size),
            instruct=instruct or None, **knobs):
        _sample_rate = sr
        a = np.asarray(chunk, dtype="float32").squeeze()
        if a.size:
            yield a, sr


def synth_checked(text: str, tries: int = 3, instruct: str = "",
                  keep_bad: bool = False, **knobs):
    """
    Synthesis with the take checked and retaken if it is no good.

    Same contract as the CosyVoice sidecar: three bad takes and the answer
    stops rather than being skipped, because a silently missing sentence still
    sounds like a complete answer.
    """
    last = "no attempt"
    for attempt in range(1, tries + 1):
        try:
            audio, sr = synth_once(text, instruct, **knobs)
        except RuntimeError as e:
            last = f"inference: {e}"
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


@app.get("/health")
def health():
    return {
        "ok": _model is not None,
        "model": CFG.model_id,
        "engine": "qwen3-tts",
        "sample_rate": _sample_rate,
        "reference": str(CFG.reference),
        "slice": {"start": CFG.start, "length": CFG.length,
                  "secs": round(_slice_secs, 2), "snap": CFG.snap},
        "prompt_text": _prompt_text,
        "stats": dict(_stats),
        "chunk_size": DEFAULTS.get("chunk_size", CFG.chunk_size),
        "vram": vram(),
    }


@app.post("/speak")
def speak(body: dict = Body(...)):
    text = (body.get("text") or "").strip()
    if not text:
        return JSONResponse({"error": "text is empty"}, status_code=400)
    mode = (body.get("mode") or "stream").lower()
    instruct = (body.get("instruct") or "").strip()
    knobs = knobs_from(body)
    chunk = body.get("chunk_size")
    try:
        chunk = int(chunk) if chunk else None
    except (TypeError, ValueError):
        chunk = None
    # only for the web page: hand back a take that failed its checks so it can
    # be listened to instead of guessed at
    keep_bad = bool(body.get("nocheck", False))

    def produce():
        with _lock:
            _stats["requests"] += 1
            t0 = time.time()

            if mode == "stream":
                sent, total = 0, 0.0
                try:
                    for audio, sr in synth_stream(text, instruct, chunk_size=chunk, **knobs):
                        # only the first chunk carries leading silence; trimming
                        # later ones would cut into the seam between them
                        if sent == 0:
                            audio = trim_head(audio, sr)
                        total += audio.size / sr
                        sent += 1
                        yield frame(wav_bytes(audio, sr))
                except RuntimeError as e:
                    print(f"stream failed after {sent} chunk(s): {e}", flush=True)
                if sent:
                    yield END
                    took = time.time() - t0
                    print(f"stream: {sent} frames, {total:.2f}s audio in {took:.2f}s "
                          f":: {text[:60]}", flush=True)
                    return
                # nothing came out - fall back rather than answer with silence,
                # and flag it so an A/B comparison stays honest
                print("stream produced no audio, falling back to one shot", flush=True)
                try:
                    audio, sr = synth_checked(text, instruct=instruct, **knobs)
                except RuntimeError as e:
                    print(f"giving up on {text[:60]!r}: {e}", flush=True)
                    return
                yield frame(wav_bytes(trim_head(audio, sr), sr), FLAG_FELL_BACK)
                yield END
                return

            try:
                audio, sr = synth_checked(
                    text, tries=1 if keep_bad else 3, instruct=instruct,
                    keep_bad=keep_bad, **knobs)
            except RuntimeError as e:
                # no frames and no END: a truncated body is how the client
                # learns the answer died, so it stops rather than pretending
                print(f"giving up on {text[:60]!r}: {e}", flush=True)
                return
            audio = trim_head(audio, sr)
            secs = audio.size / sr
            took = time.time() - t0
            print(f"sentence: {secs:.2f}s audio in {took:.2f}s "
                  f"(rtf {took/max(secs,1e-6):.2f}) :: {text[:60]}", flush=True)
            yield frame(wav_bytes(audio, sr))
            yield END

    return StreamingResponse(produce(), media_type="application/octet-stream")


@app.get("/config")
def api_config_get():
    return {"defaults": DEFAULTS, "builtin": KNOB_DEFAULTS,
            "reference": {"path": str(CFG.reference), "start": CFG.start,
                          "length": CFG.length, "snap": CFG.snap,
                          "secs": round(_slice_secs, 2), "text": _prompt_text},
            "language": CFG.language, "saved_to": str(CONFIG_PATH)}


@app.post("/config")
def api_config_set(body: dict = Body(...)):
    """
    Change what a request gets when it does not say.

    Jarvis never names a sampling value, so this is the only place its answers
    can be tuned from. Written to disk so a restart does not quietly undo an
    afternoon of listening.
    """
    if body.get("reset"):
        CONFIG_PATH.unlink(missing_ok=True)
        load_config()
        return {"defaults": DEFAULTS, "reset": True}

    changed = {}
    for k, default in KNOB_DEFAULTS.items():
        if k not in body:
            continue
        try:
            v = int(body[k]) if isinstance(default, int) else float(body[k])
        except (TypeError, ValueError):
            return JSONResponse({"error": f"{k}: expected a number, got {body[k]!r}"},
                                status_code=400)
        DEFAULTS[k] = v
        changed[k] = v
    if "chunk_size" in body:
        try:
            DEFAULTS["chunk_size"] = max(1, int(body["chunk_size"]))
            changed["chunk_size"] = DEFAULTS["chunk_size"]
        except (TypeError, ValueError):
            return JSONResponse({"error": "chunk_size: expected a whole number"},
                                status_code=400)
    save_config()
    print("defaults changed: " + ", ".join(f"{k}={v}" for k, v in changed.items()),
          flush=True)
    return {"defaults": DEFAULTS, "changed": changed}


@app.post("/reference")
def api_reference(body: dict = Body(...)):
    """
    Re-cut the reference and adopt it without restarting.

    This one IS state the loaded model uses: the slice and its transcript are
    what the voice is cloned from, so changing them changes how the assistant
    sounds from the next sentence onward.

    The transcript has to match the audio word for word. It is looked up by
    the audio's own bytes first, then taken from the request, and only then
    from whisper - which this environment deliberately does not have, so a
    brand new slice needs its text typed in.
    """
    ref = Path((body.get("path") or str(CFG.reference)).strip())
    if not ref.exists():
        return JSONResponse({"error": f"no such file: {ref}"}, status_code=400)
    try:
        start = float(body.get("start", CFG.start))
        length = float(body.get("length", CFG.length))
    except (TypeError, ValueError):
        return JSONResponse({"error": "start and length must be numbers"}, status_code=400)
    snap = bool(body.get("snap", CFG.snap))
    given = (body.get("text") or "").strip()

    global _slice_path, _slice_secs, _prompt_text
    with _lock:
        out = HERE / "studio_out" / "_sidecar_qwen_ref.wav"
        path, secs = refslice.slice_reference(ref, start, length, out, do_snap=snap)
        cache = transcript_cache(path, HERE)
        if cache.exists():
            text, source = cache.read_text(encoding="utf-8").strip(), "cache"
        elif given:
            cache.write_text(given, encoding="utf-8")
            text, source = given, "given"
        else:
            try:
                text, source = transcribe(path), "whisper"
            except ImportError:
                return JSONResponse(
                    {"error": "this slice has no transcript yet and whisper is not "
                              "installed here. Type the words that are spoken in it."},
                    status_code=400)
        CFG.reference, CFG.start, CFG.length, CFG.snap = ref, start, length, snap
        _slice_path, _slice_secs, _prompt_text = path, secs, text
        # to disk immediately: this is the setting people actually notice
        # losing, because losing it changes the voice
        save_config()

    print(f"reference now {ref.name} {start}+{length}s -> {secs:.2f}s "
          f"(transcript from {source})", flush=True)
    return {"reference": str(ref), "start": start, "length": length, "snap": snap,
            "secs": round(secs, 2), "text": text, "transcript_from": source}


@app.get("/reference/audio")
def api_reference_audio():
    """the slice itself, to hear what the voice is being cloned from"""
    return Response(Path(_slice_path).read_bytes(), media_type="audio/wav")


@app.get("/models")
def api_models():
    return {"current": CFG.model_id, "known": KNOWN_MODELS}


@app.post("/model")
def api_model(body: dict = Body(...)):
    """
    Swap the model without restarting.

    Blocks until the new one is loaded, which on a first run means downloading
    it - about 3.5 GB for the 1.7B. The old one is dropped first so the two
    never sit in video memory together; on a 16 GB card shared with a language
    model, they would not fit.

    If the new model fails to load, the old id is put back so the next request
    does not keep trying something broken.
    """
    wanted = (body.get("model_id") or "").strip()
    if not wanted:
        return JSONResponse({"error": "model_id is empty"}, status_code=400)
    if wanted == CFG.model_id and _model is not None:
        return {"model": wanted, "changed": False, "vram": vram()}

    previous = CFG.model_id
    with _lock:
        unload()
        CFG.model_id = wanted
        t0 = time.time()
        try:
            model()
        except Exception as e:
            CFG.model_id = previous
            print(f"could not load {wanted}: {e}", flush=True)
            return JSONResponse(
                {"error": f"{type(e).__name__}: {e}", "model": previous},
                status_code=400)
        took = time.time() - t0
        save_config()

        # warm the cloning path, same reason as at startup
        try:
            for _ in synth_stream("Готово."):
                pass
        except Exception as e:
            print(f"warm-up after the swap failed: {e}", flush=True)

    known = next((m for m in KNOWN_MODELS if m["id"] == wanted), None)
    print(f"model is now {wanted} (loaded in {took:.1f}s)", flush=True)
    return {"model": wanted, "changed": True, "took_secs": round(took, 1),
            "clones": known["clones"] if known else None, "vram": vram()}


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


# ---------------------------------------------------------------- web page
# The sidecar serves its own console. Not a separate tool on purpose: the
# point of moving temperature or chunk size is to hear THIS process, with this
# reference and this warm model, rather than a copy of it that might differ in
# some way nobody thought to check.
PAGE = """
<!doctype html><html lang="ru"><head><meta charset="utf-8">
<title>Jarvis TTS</title>
<style>
:root{--bg:#0d1117;--card:#161b22;--line:#26303b;--ink:#e2e8ec;--soft:#8b98a5;--acc:#5ad1c4;--warn:#e0a458}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--ink);font:15px/1.55 system-ui,sans-serif;padding:26px}
.wrap{max-width:960px;margin:0 auto}
h1{font-size:19px;margin:0 0 3px;letter-spacing:-.01em}
.sub{color:var(--soft);font-size:13px;margin-bottom:20px}
.card{background:var(--card);border:1px solid var(--line);border-radius:8px;padding:17px;margin-bottom:15px}
label{display:block;font-size:11.5px;text-transform:uppercase;letter-spacing:.07em;color:var(--soft);margin-bottom:5px}
input,textarea,select{width:100%;background:#0d1117;color:var(--ink);border:1px solid var(--line);
  border-radius:5px;padding:8px 10px;font:inherit}
textarea{resize:vertical;min-height:62px}
.row{display:flex;gap:11px;flex-wrap:wrap}
.row>div{flex:1;min-width:105px}
button{background:var(--acc);color:#06231f;border:0;border-radius:5px;padding:9px 17px;font:600 14px system-ui;cursor:pointer}
button.ghost{background:transparent;color:var(--acc);border:1px solid var(--line)}
button:disabled{opacity:.45;cursor:default}
.bar{display:flex;gap:9px;align-items:center;margin-top:13px;flex-wrap:wrap}
.meta{color:var(--soft);font-size:12.5px;font-variant-numeric:tabular-nums}
.warn{color:var(--warn)}
.item{border-top:1px solid var(--line);padding:11px 0;display:flex;gap:11px;align-items:center;flex-wrap:wrap}
.item .txt{flex:1;min-width:190px}
audio{height:34px}
code{background:#0d1117;padding:1px 5px;border-radius:3px;font-size:12.5px}
</style></head><body><div class="wrap">
<h1>Jarvis TTS</h1>
<div class="sub" id="banner">Qwen3-TTS. Модель держится загруженной, одна генерация за раз.</div>

<div class="card">
  <div class="bar" style="margin-top:0">
    <button class="ghost" onclick="adm('/reload')">Загрузить в память</button>
    <button class="ghost" onclick="adm('/unload')">Выгрузить из памяти</button>
    <span id="vram" class="meta"></span>
  </div>
</div>

<div class="card">
  <label>Текст</label>
  <textarea id="text">Система работает штатно, сэр. Все процессы в пределах нормы.</textarea>
  <div style="margin-top:11px">
    <label>Инструкция — как говорить</label>
    <input id="instruct" placeholder="пусто = обычное клонирование по образцу">
    <div class="meta warn" style="margin-top:5px">Модель часто зачитывает инструкцию вслух вместо того,
      чтобы ей следовать. Проверено на русском, английском и китайском: надёжно работает только пустое поле.</div>
  </div>
</div>

<div class="card">
  <div class="row">
    <div><label>Температура</label><input id="temperature" type="number" step="0.05" value="0.9"></div>
    <div><label>top_k</label><input id="top_k" type="number" step="1" value="50"></div>
    <div><label>top_p</label><input id="top_p" type="number" step="0.05" value="1.0"></div>
    <div><label>Штраф за повтор</label><input id="repetition_penalty" type="number" step="0.01" value="1.05"></div>
  </div>
  <div class="row" style="margin-top:11px">
    <div><label>Кусок потока</label><input id="chunk_size" type="number" step="1" value="12"></div>
    <div><label>Максимум токенов</label><input id="max_new_tokens" type="number" step="128" value="2048"></div>
    <div><label>Режим</label><select id="mode">
      <option value="stream">потоковый</option><option value="sentence">целиком</option></select></div>
  </div>
  <div class="meta" style="margin-top:8px">Кусок потока — примерно <code>размер / 12</code> секунды звука.
    Меньше — первый звук раньше, но вокодер работает чаще.</div>
  <div class="meta" style="margin-top:6px">Это параметры <b>каждого вызова</b>, а не загруженной модели —
    веса от них не меняются. Джарвис их не присылает, поэтому его ответы получают то,
    что здесь <b>сохранено</b>. Пока не нажмёшь «Сохранить», настройки влияют только на пробы.</div>
  <div class="bar">
    <button id="go" onclick="gen()">Сгенерировать</button>
    <button class="ghost" onclick="saveCfg()">Сохранить как умолчания</button>
    <button class="ghost" onclick="resetCfg()">Сбросить</button>
    <label style="display:inline;text-transform:none;letter-spacing:0;color:var(--ink)">
      <input type="checkbox" id="nocheck" style="width:auto"> отдавать даже забракованный дубль
    </label>
    <span id="status" class="meta"></span>
  </div>
</div>

<div class="card">
  <label>Модель синтеза</label>
  <div class="row" style="margin-top:2px">
    <div style="flex:3"><select id="model_id"></select></div>
    <div style="flex:1;min-width:150px"><button class="ghost" style="width:100%"
      onclick="applyModel()">Загрузить модель</button></div>
  </div>
  <div class="meta" id="model_note" style="margin-top:6px"></div>
  <div class="meta warn" style="margin-top:5px">Голос по образцу клонируют только варианты
    <code>-Base</code>. CustomVoice и VoiceDesign говорят своими голосами и твой эталон
    игнорируют. Первая загрузка новой модели качает несколько гигабайт и занимает минуты.</div>
</div>

<div class="card">
  <label>Эталон голоса</label>
  <div class="meta" style="margin-bottom:9px">А вот это — настоящее состояние загруженной модели:
    из этого отрезка клонируется голос. Меняется на лету, со следующей же фразы.</div>
  <input id="ref_path" placeholder="путь к wav">
  <div class="row" style="margin-top:10px">
    <div><label>Начало, с</label><input id="ref_start" type="number" step="0.5" value="5"></div>
    <div><label>Длина, с</label><input id="ref_length" type="number" step="1" value="8"></div>
    <div><label>По паузам</label><select id="ref_snap">
      <option value="1">да</option><option value="0">нет</option></select></div>
  </div>
  <div style="margin-top:10px">
    <label>Расшифровка отрезка</label>
    <textarea id="ref_text" style="min-height:52px"></textarea>
    <div class="meta" style="margin-top:5px">Должна совпадать с отрезком слово в слово: модель
      продолжает эту речь твоим текстом. Для нового отрезка её надо вписать — whisper в это
      окружение намеренно не ставился.</div>
  </div>
  <div class="bar">
    <button class="ghost" onclick="applyRef()">Применить эталон</button>
    <button class="ghost" onclick="hearRef()">Послушать отрезок</button>
    <span id="refmeta" class="meta"></span>
  </div>
  <div style="margin-top:9px"><audio id="refaudio" controls style="width:100%"></audio></div>
</div>

<div class="card"><div id="list"></div></div>
</div>
<script>
const $=id=>document.getElementById(id)
async function refresh(){
  try{
    const h=await (await fetch('/health')).json()
    const v=h.vram||{}
    $('vram').textContent=(h.ok?'в памяти':'выгружена')
      +(v.free_mb?(' \u00b7 свободно '+v.free_mb+' из '+v.total_mb+' МБ'):'')
    $('banner').textContent=h.model+' \u00b7 эталон '+h.slice.secs+'s \u00b7 '+(h.sample_rate||'?')+' Гц'
  }catch(e){ $('vram').textContent='сайдкар не отвечает' }
}
async function adm(path){
  $('vram').textContent='...'
  try{ await fetch(path,{method:'POST'}) } finally { refresh() }
}
function body(){
  return {text:$('text').value, mode:$('mode').value, instruct:$('instruct').value,
    temperature:+$('temperature').value, top_k:+$('top_k').value, top_p:+$('top_p').value,
    repetition_penalty:+$('repetition_penalty').value, chunk_size:+$('chunk_size').value,
    max_new_tokens:+$('max_new_tokens').value, nocheck:$('nocheck').checked}
}
async function gen(){
  const btn=$('go'), st=$('status')
  btn.disabled=true; st.className='meta'; st.textContent='генерация...'
  const t0=performance.now()
  try{
    const r=await fetch('/speak',{method:'POST',headers:{'Content-Type':'application/json'},
      body:JSON.stringify(body())})
    if(!r.ok){ st.className='meta warn'; st.textContent='ошибка '+r.status; return }
    const buf=new Uint8Array(await r.arrayBuffer())
    const view=new DataView(buf.buffer)
    const parts=[]; let off=0, ended=false
    while(off+8<=buf.length){
      const len=view.getUint32(off,true); off+=8
      if(len===0){ ended=true; break }
      if(off+len>buf.length) break
      parts.push(buf.slice(off,off+len)); off+=len
    }
    const took=Math.round(performance.now()-t0)
    if(!parts.length){
      st.className='meta warn'
      st.textContent=took+' мс \u2014 звука нет: проверка забраковала все дубли'
      return
    }
    st.textContent=took+' мс \u00b7 '+parts.length+' кадр(ов)'+(ended?'':' \u00b7 ОБОРВАНО')
    addItem($('text').value, parts, took, ended)
  }catch(e){ st.className='meta warn'; st.textContent=String(e) }
  finally{ btn.disabled=false; refresh() }
}
function addItem(text, parts, took, ended){
  const urls=parts.map(p=>URL.createObjectURL(new Blob([p],{type:'audio/wav'})))
  const el=document.createElement('div'); el.className='item'
  const t=document.createElement('div'); t.className='txt'
  t.textContent=text
  const m=document.createElement('div'); m.className='meta'
  m.textContent=took+' мс \u00b7 '+parts.length+' кадр(ов)'+(ended?'':' \u00b7 оборвано')
  t.appendChild(m); el.appendChild(t)
  const a=document.createElement('audio'); a.controls=true; a.src=urls[0]
  let i=0
  a.onended=()=>{ if(++i<urls.length){ a.src=urls[i]; a.play() } }
  el.appendChild(a)
  const dl=document.createElement('a')
  dl.textContent='скачать'; dl.href=urls[0]; dl.download='tts.wav'
  dl.style.cssText='color:var(--acc);font-size:12.5px'
  el.appendChild(dl)
  $('list').prepend(el)
}
let MODELS=[]
async function loadModels(){
  try{
    const d=await (await fetch('/models')).json()
    MODELS=d.known
    const sel=$('model_id')
    sel.innerHTML=MODELS.map(m=>
      '<option value="'+m.id+'"'+(m.id===d.current?' selected':'')+'>'
      +m.id.replace('Qwen/Qwen3-TTS-12Hz-','')+(m.clones?'':'  \u2014 без клонирования')
      +'</option>').join('')
    showModelNote()
  }catch(e){}
}
function showModelNote(){
  const m=MODELS.find(x=>x.id===$('model_id').value)
  $('model_note').textContent=m?m.note:''
  $('model_note').className='meta'+(m&&!m.clones?' warn':'')
}
async function applyModel(){
  const id=$('model_id').value
  const m=MODELS.find(x=>x.id===id)
  if(m && !m.clones &&
     !confirm('Эта модель не клонирует голос по образцу. Джарвис будет говорить '
              +'чужим голосом. Всё равно загрузить?')) return
  const note=$('model_note')
  note.className='meta'; note.textContent='загружаю... первая загрузка качает гигабайты'
  try{
    const r=await fetch('/model',{method:'POST',headers:{'Content-Type':'application/json'},
      body:JSON.stringify({model_id:id})})
    const d=await r.json()
    if(d.error){ note.className='meta warn'; note.textContent=d.error; return }
    note.textContent=d.changed?('загружена за '+d.took_secs+' с'):'уже была загружена'
  }catch(e){ note.className='meta warn'; note.textContent=String(e) }
  finally{ refresh() }
}

const KNOBS=['temperature','top_k','top_p','repetition_penalty','max_new_tokens','chunk_size']
async function loadCfg(){
  try{
    const c=await (await fetch('/config')).json()
    for(const k of KNOBS) if(k in c.defaults && $(k)) $(k).value=c.defaults[k]
    const r=c.reference||{}
    $('ref_path').value=r.path||''
    $('ref_start').value=r.start
    $('ref_length').value=r.length
    $('ref_snap').value=r.snap?'1':'0'
    $('ref_text').value=r.text||''
    $('refmeta').textContent='отрезок '+(r.secs||'?')+' с'
  }catch(e){}
}
async function saveCfg(){
  const b={}
  for(const k of KNOBS) if($(k)) b[k]=+$(k).value
  const st=$('status')
  const r=await fetch('/config',{method:'POST',headers:{'Content-Type':'application/json'},
    body:JSON.stringify(b)})
  const d=await r.json()
  if(d.error){ st.className='meta warn'; st.textContent=d.error; return }
  st.className='meta'
  st.textContent='сохранено — теперь эти значения получают и ответы Джарвиса'
}
async function resetCfg(){
  await fetch('/config',{method:'POST',headers:{'Content-Type':'application/json'},
    body:JSON.stringify({reset:true})})
  await loadCfg()
  $('status').className='meta'; $('status').textContent='сброшено к заводским'
}
async function applyRef(){
  const st=$('refmeta'); st.className='meta'; st.textContent='режу и применяю...'
  const b={path:$('ref_path').value, start:+$('ref_start').value,
    length:+$('ref_length').value, snap:$('ref_snap').value==='1', text:$('ref_text').value}
  const r=await fetch('/reference',{method:'POST',headers:{'Content-Type':'application/json'},
    body:JSON.stringify(b)})
  const d=await r.json()
  if(d.error){ st.className='meta warn'; st.textContent=d.error; return }
  $('ref_text').value=d.text
  st.className='meta'
  st.textContent='отрезок '+d.secs+' с \u00b7 расшифровка из: '+d.transcript_from
  hearRef()
}
function hearRef(){ $('refaudio').src='/reference/audio?'+Date.now() }
loadModels()
loadCfg()
$('model_id').addEventListener('change', showModelNote)
refresh(); setInterval(refresh, 5000)
</script></body></html>
"""


@app.get("/", response_class=HTMLResponse)
def index():
    return PAGE


def main():
    global CFG, _slice_path, _slice_secs, _prompt_text
    ap = argparse.ArgumentParser()
    ap.add_argument("--host", default="127.0.0.1")
    # a different port from the CosyVoice sidecar on purpose: both can run, and
    # switching engines is then one setting rather than a restart dance
    ap.add_argument("--port", type=int, default=8772)
    ap.add_argument("--model-id", default="Qwen/Qwen3-TTS-12Hz-0.6B-Base")
    ap.add_argument("--language", default="Russian")
    ap.add_argument("--reference", default=str(HERE / "xamples" / "jarvis_sample.wav"))
    # the same slice the voice pack was baked from; changing it changes the voice
    ap.add_argument("--start", type=float, default=5.0)
    ap.add_argument("--length", type=float, default=8.0)
    ap.add_argument("--no-snap", dest="snap", action="store_false", default=True)
    # the transcript of the slice. Passed in rather than recomputed: whisper is
    # a heavy dependency to duplicate into a second environment for a value the
    # other sidecar has already worked out from the identical slice.
    ap.add_argument("--prompt-text", default="")
    # codec steps per streamed chunk; roughly chunk_size/12 seconds of audio.
    # Smaller means the first sound arrives sooner and the vocoder runs more
    # often - the same trade CosyVoice makes with token_hop_len.
    ap.add_argument("--chunk-size", type=int, default=12)
    # saved settings beat the command line, so there has to be a way to say
    # "no, actually, start over"
    ap.add_argument("--reset-config", action="store_true",
                    help="ignore and delete the saved settings")
    a = ap.parse_args()
    a.reference = Path(a.reference)
    CFG = a

    # before the slice is cut, so a saved reference is the one that gets cut
    if a.reset_config:
        CONFIG_PATH.unlink(missing_ok=True)
        print(f"{CONFIG_PATH.name} discarded, starting from the command line", flush=True)
    load_config()

    if not a.reference.exists():
        print(f"reference not found: {a.reference}", file=sys.stderr)
        return 2

    # a model chosen from the console outlives the process too, same rule as
    # the reference: what was set there wins until --reset-config
    if DEFAULTS.get("model_id"):
        a.model_id = DEFAULTS["model_id"]

    # a reference set from the console outlives the process; the command line
    # is only the starting point. --reset-config goes back to it.
    if SAVED_REFERENCE:
        saved_path = Path(SAVED_REFERENCE["path"])
        if saved_path.exists():
            a.reference = saved_path
            a.start = float(SAVED_REFERENCE.get("start", a.start))
            a.length = float(SAVED_REFERENCE.get("length", a.length))
            a.snap = bool(SAVED_REFERENCE.get("snap", a.snap))
        else:
            print(f"saved reference {saved_path} is gone, using the command line",
                  flush=True)

    out = HERE / "studio_out" / "_sidecar_qwen_ref.wav"
    out.parent.mkdir(exist_ok=True)
    _slice_path, _slice_secs = refslice.slice_reference(
        a.reference, a.start, a.length, out, do_snap=a.snap)
    print(f"reference : {a.reference.name}  {a.start}+{a.length}s "
          f"-> {_slice_secs:.2f}s  snap={a.snap}")
    _prompt_text = a.prompt_text.strip() or transcribe(_slice_path)
    print(f"transcript: {_prompt_text}")

    model()                  # warm before serving

    # ...and warm the path that will actually be used. The model's own
    # warmup() does not touch voice cloning, so the first cloned request still
    # cost 5.7 s against the 0.43 s every one after it - and the first request
    # is precisely the one somebody is watching. One throwaway line here moves
    # that cost off the first question.
    try:
        t0 = time.time()
        for _ in synth_stream("Готово."):
            pass
        print(f"cloning path warmed in {time.time()-t0:.1f}s", flush=True)
    except Exception as e:
        print(f"warm-up generation failed, first answer will be slow: {e}", flush=True)
    print(f"http://{a.host}:{a.port}", flush=True)

    import uvicorn
    uvicorn.run(app, host=a.host, port=a.port, log_level="warning")
    return 0


if __name__ == "__main__":
    sys.exit(main())
