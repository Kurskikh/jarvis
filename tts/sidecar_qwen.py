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

    python sidecar_qwen.py --reference "xamples/jarvis_sample.wav"
"""
import argparse
import csv
import io
import json
import os
import re
import subprocess
import sys
import threading
import time
from pathlib import Path

HERE = Path(__file__).parent
# Model caches live with every other model, one folder up. Set before any
# huggingface import: these are read once, at import time, and a late change
# has no effect at all.
CACHE = HERE.parent / "models" / "sidecar"
os.environ.setdefault("HF_HOME", str(CACHE / "hf"))

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
# separate from _lock and re-entrant: loading is called from inside paths
# that already hold _lock, and from request threads that do not
_load_lock = threading.RLock()
_slice_path = None
_slice_secs = 0.0
_prompt_text = ""
_sample_rate = None
_stats = {"requests": 0, "retakes": 0, "failures": 0}


def model():
    """
    The one model. There is never a second.

    Guarded because it was not: two requests arriving before the first load
    finished would both see None and both call from_pretrained, and the card
    would hold two copies while the variable pointed at one. The lock is
    re-entrant so the paths that already hold it - the swap, the warm-up - can
    call this without deadlocking.
    """
    global _model
    with _load_lock:
        return _load_locked()


# What the loader is doing, for a page that would otherwise show nothing.
#
# Loading takes the better part of a minute, and the first time a model is
# chosen it is several gigabytes off the network. The button used to block on
# all of that with no sign of life - and worse, the five-second refresh wiped
# the one "..." that marked it, so pressing "Загрузить в память" looked exactly
# like pressing nothing at all.
LOAD = {"busy": False, "phase": "", "started": 0.0, "model": "",
        "error": "", "bytes": 0, "took": 0.0}


def _cache_bytes(model_id):
    """
    How much of this model is on disk, for the first-run download.

    There is no percentage to be had - nothing tells us the total before it
    arrives - but a number that climbs is the difference between waiting and
    wondering whether it hung.
    """
    folder = "models--" + model_id.replace("/", "--")
    # blobs only. Every file under snapshots/ is a hard link to one of these,
    # so walking the whole tree counts each weight twice - which it did, and
    # reported a 4.2 GB model as 8.5 GB on disk.
    root = Path(os.environ.get("HF_HOME", "")) / "hub" / folder / "blobs"
    if not root.exists():
        return 0
    total = 0
    for p in root.rglob("*"):
        try:
            if p.is_file():
                total += p.stat().st_size
        except OSError:
            pass
    return total


def _watch_download(model_id, stop):
    """Report the growing cache while from_pretrained is fetching."""
    while not stop.is_set():
        LOAD["bytes"] = _cache_bytes(model_id)
        stop.wait(1.5)


def _load_locked():
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

        cached = _cache_bytes(CFG.model_id)
        LOAD.update(busy=True, model=CFG.model_id, started=t0, error="",
                    bytes=cached,
                    phase="скачиваю веса" if cached < 100 * 2**20 else "читаю веса")
        stop = threading.Event()
        watcher = threading.Thread(target=_watch_download, args=(CFG.model_id, stop),
                                   daemon=True)
        watcher.start()
        try:
            _model = FasterQwen3TTS.from_pretrained(
                CFG.model_id,
                device="cuda" if torch.cuda.is_available() else "cpu",
                dtype=torch.bfloat16,
                attn_implementation="sdpa",
            )
        except Exception as e:
            LOAD.update(busy=False, phase="", error=f"{type(e).__name__}: {e}")
            raise
        finally:
            stop.set()

        LOAD["phase"] = "прогреваю"
        try:
            _model.warmup()
        except Exception as e:
            # not fatal: it only costs the first request some extra latency
            print(f"warmup skipped: {e}", flush=True)

        took = time.time() - t0
        LOAD.update(busy=False, phase="", took=round(took, 1))
        print(f"model loaded in {took:.1f}s", flush=True)
    return _model


def unload():
    """
    Drop the model and hand the video memory back.

    Three models want this card at once - the language model in LM Studio, the
    other speech sidecar, and this one - and 16 GB does not stretch forever.
    Whichever is idle should be the one to let go.
    """
    global _model
    # the same lock the load takes: dropping the model while another thread is
    # halfway through creating one would leave the new copy unreferenced and
    # the card holding it anyway
    with _load_lock:
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
    """
    Four numbers, because two of them were being read as one.

    "used_by_us" used to mean memory_allocated(), which counts live tensors and
    nothing else. PyTorch keeps a much larger pool reserved and hands none of it
    back to the driver, so that figure read as a few gigabytes while the card
    was full - and the card's own "free" disagreed with nvidia-smi by about a
    gigabyte for the same reason. Neither was wrong; they answer different
    questions, and only one of them was on screen.

    So: what the card says, and what this process holds, separately - and for
    this process both the live tensors and the pool they came out of.
    """
    if not torch.cuda.is_available():
        return None
    free, total = torch.cuda.mem_get_info()
    mb = lambda b: round(b / 2**20)
    return {
        "card_total_mb": mb(total),
        "card_free_mb": mb(free),
        "card_used_mb": mb(total - free),
        # live tensors
        "ours_live_mb": mb(torch.cuda.memory_allocated()),
        # everything torch has taken from the driver and not given back. This
        # is the number that matters when something else wants the card.
        "ours_held_mb": mb(torch.cuda.memory_reserved()),
    }


# ------------------------------------------------------------ the voice pack

# Where Jarvis keeps the clips it plays without asking the model: "да, сэр" when
# the name is heard, "думаю над ответом" while an answer is on its way. They are
# baked from the same reference the live voice clones from, so a new reference
# means they no longer match the voice - which is exactly the job that had no
# interface until now and lived in a script nobody could run without whisper.
VOICES_DIR = HERE.parent / "resources" / "sound" / "voices"

# Which pack the Заготовки tab is working on. A whole voice at a time: the
# clips only make sense together, and baking half of one pack with the
# reference of another is how a voice ends up sounding like two people.
PACK_ID = "jarvis-og-tts"
PACK_LANG = "ru"


def _pack_dir():
    return VOICES_DIR / PACK_ID


def _pack_langs(doc):
    langs = (doc.get("voice", {}) or {}).get("languages", []) or []
    return [str(x) for x in langs] or ["ru"]


def list_packs():
    """
    Every voice on disk, whether it has any clips yet or not.

    A pack with no recordings is a real state and worth showing: it is what a
    voice looks like between being created and being baked.
    """
    out = []
    if not VOICES_DIR.exists():
        return out
    import tomlkit
    for folder in sorted(VOICES_DIR.iterdir()):
        toml = folder / "voice.toml"
        if not folder.is_dir() or not toml.exists():
            continue
        try:
            doc = tomlkit.parse(toml.read_text(encoding="utf-8"))
        except Exception:
            continue
        meta = doc.get("voice", {}) or {}
        langs = _pack_langs(doc)
        clips = sum(1 for lang in langs for _ in (folder / lang).glob("*.wav")
                    if (folder / lang).exists())
        lines = len((doc.get("lines", {}) or {}).get(PACK_LANG, {}) or {})
        out.append({
            "id": str(meta.get("id", folder.name)),
            "name": str(meta.get("name", folder.name)),
            "author": str(meta.get("author", "")),
            "languages": langs,
            "clips": clips,
            "lines": lines,
            "current": folder.name == PACK_ID,
            "folder": folder.name,
        })
    return out


def _pack_doc():
    """voice.toml, parsed so it can be written back with its comments intact."""
    import tomlkit
    path = _pack_dir() / "voice.toml"
    return tomlkit.parse(path.read_text(encoding="utf-8")), path


def _clip_path(stem):
    """
    The file for one clip, or None if the name is not a plain stem.

    A stem arrives over HTTP and is turned into a path, so it is checked rather
    than trusted: without this, "../../../something" would be a way to read and
    overwrite files outside the pack.
    """
    stem = (stem or "").strip()
    if not stem or stem.startswith((".", "_")) or len(stem) > 64:
        return None
    if any(c in stem for c in '/' + chr(92) + ':*?"<>|'):
        return None
    return _pack_dir() / PACK_LANG / (stem + ".wav")


def _clip_seconds(path):
    try:
        info = sf.info(str(path))
        return round(info.frames / info.samplerate, 2)
    except Exception:
        return None


def _pack_lines(doc):
    table = doc.get("lines", {})
    return (table.get(PACK_LANG, {}) if table else {}) or {}


def pack_rows():
    """
    Every clip in the pack, in the order the reactions declare them.

    A stem can be named by a reaction with no file yet, and a file can sit in
    the folder with no reaction pointing at it. Both are listed: the first is
    something to bake, the second is something nobody ever plays.
    """
    doc, _ = _pack_doc()
    reactions = (doc.get("reactions", {}) or {}).get(PACK_LANG, {}) or {}
    lines = _pack_lines(doc)
    folder = _pack_dir() / PACK_LANG

    rows, seen = [], set()
    for reaction, stems in reactions.items():
        for stem in stems:
            stem = str(stem)
            if stem in seen:
                continue
            seen.add(stem)
            wav = folder / (stem + ".wav")
            rows.append({
                "stem": stem,
                "reaction": str(reaction),
                "text": str(lines.get(stem, "")),
                "exists": wav.exists(),
                "secs": _clip_seconds(wav) if wav.exists() else None,
                "orphan": False,
            })

    if folder.exists():
        for wav in sorted(folder.glob("*.wav")):
            if wav.stem in seen or wav.stem.startswith("_"):
                continue
            rows.append({
                "stem": wav.stem, "reaction": "", "text": str(lines.get(wav.stem, "")),
                "exists": True, "secs": _clip_seconds(wav), "orphan": True,
            })

    return rows


def _set_line(doc, stem, text):
    import tomlkit
    if "lines" not in doc:
        doc["lines"] = tomlkit.table(True)
    if PACK_LANG not in doc["lines"]:
        doc["lines"][PACK_LANG] = tomlkit.table()
    doc["lines"][PACK_LANG][stem] = text


# ------------------------------------------------------- who holds the card

# The address this is actually listening on, filled in by main().
#
# It matters because one endpoint below ends processes. On the loopback that is
# a console for the person at the keyboard; on 0.0.0.0 it is the same console
# for everyone on the network.
BOUND_HOST = "127.0.0.1"
LOOPBACK = {"127.0.0.1", "::1", "localhost"}

# Never offered for killing, whatever they are holding.
#
# dwm comes first by usage on every Windows machine, and it is the desktop
# itself. Windows does restart most of these, but "restarts" means every window
# on screen is torn down and rebuilt, which is not what a button should do by
# surprise. The rest are the session and the kernel.
PROTECTED = {
    "dwm", "explorer", "csrss", "winlogon", "wininit", "services", "lsass",
    "smss", "system", "registry", "idle", "system idle process",
    "memory compression", "lsaiso", "fontdrvhost", "sihost",
}

# instance names look like  pid_10976_luid_0x00000000_0x0001CE17_phys_0
_PID_IN_INSTANCE = re.compile(r"pid_(\d+)_", re.IGNORECASE)

# no console window when this is launched from a windowless parent
_NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)


def _dedicated_by_pid():
    """
    Bytes of dedicated GPU memory per process id, from Windows' own counters.

    NOT from nvidia-smi. On Windows the display driver runs in WDDM mode, and
    there the driver does not report per-process memory at all: every row of
    --query-compute-apps=used_memory comes back "[N/A]", for all fifty-odd
    processes that have touched the card. The performance counters do have the
    figure, and they are the ones Task Manager draws.

    Returns {} on anything that is not Windows, or if the counter is missing.
    """
    if os.name != "nt":
        return {}

    try:
        done = subprocess.run(
            ["typeperf", r"\GPU Process Memory(*)\Dedicated Usage", "-sc", "1"],
            capture_output=True, text=True, encoding="utf-8", errors="replace",
            timeout=25, creationflags=_NO_WINDOW,
        )
    except (OSError, subprocess.SubprocessError):
        return {}

    rows = [r for r in csv.reader(io.StringIO(done.stdout)) if len(r) > 1]
    if len(rows) < 2:
        return {}

    header, values = rows[0], rows[1]
    out = {}
    # column 0 of both is the timestamp
    for name, value in zip(header[1:], values[1:]):
        found = _PID_IN_INSTANCE.search(name)
        if not found:
            continue
        try:
            taken = float(value)
        except ValueError:
            continue
        if taken <= 0:
            continue
        # A process can appear once per graphics adapter. Summing is right for
        # the question being asked - how much is this program holding - even
        # though it means the number spans adapters on a machine with two.
        out[int(found.group(1))] = out.get(int(found.group(1)), 0) + taken

    return out


def gpu_processes():
    """
    Everything currently holding dedicated GPU memory, largest first.

    These figures do not add up to the card's total, and are not meant to: dwm
    holds the composition surfaces for every window on the desktop, so memory
    an application allocated is counted once under that application and again
    under dwm. The card's own total comes from the driver, in vram(), and the
    two are shown side by side rather than reconciled.
    """
    import psutil

    ours = os.getpid()
    out = []

    for pid, taken in _dedicated_by_pid().items():
        name, cmd = "(процесс закрылся)", ""
        try:
            p = psutil.Process(pid)
            name = p.name()
            # the executable is enough to recognise something; the full command
            # line of a python process is a wall of paths
            cmd = (p.exe() or "")
        except Exception:
            pass

        stem = name.lower().removesuffix(".exe")
        if pid == ours:
            why = "self"
        elif stem in PROTECTED:
            why = "protected"
        else:
            why = None

        out.append({
            "pid": pid,
            "name": name,
            "path": cmd,
            "mb": round(taken / 2**20),
            "blocked": why,
        })

    out.sort(key=lambda r: r["mb"], reverse=True)
    return out


def may_kill(row):
    """Why this process must not be ended, or None if it may be."""
    if row["blocked"] == "self":
        return ("Это сам сайдкар. Чтобы освободить память, выгрузите модель "
                "кнопкой на вкладке синтеза.")
    if row["blocked"] == "protected":
        return ("{} — служебный процесс Windows. Его завершение обрушит "
                "рабочий стол или систему.".format(row["name"]))
    return None


def transcribe(path):
    cache = transcript_cache(path, HERE)
    if cache.exists():
        return cache.read_text(encoding="utf-8").strip()
    try:
        import whisper
    except ModuleNotFoundError:
        raise SystemExit(
            "The reference transcript is missing and whisper is not installed "
            "here, so it cannot be made. "
            f"Expected it at: {cache}. "
            "Put the text there by hand, or pass --prompt-text. "
            "That cache file is NOT disposable: deleting it takes the sidecar "
            "down until whisper is available or the text is restored."
        )
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
# What the models will accept. "Auto" lets the model decide from the text,
# which is the useful one here: answers come back with English words in them -
# model names, file paths, the odd borrowed term - and read as Russian they
# come out mangled.
SUPPORTED_LANGUAGES = [
    "Auto", "Russian", "English", "Chinese", "Japanese", "Korean",
    "German", "French", "Portuguese", "Spanish", "Italian",
]

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
# likewise for the language and the built-in voice
SAVED_LANGUAGE = None
SAVED_SPEAKER = None


def load_config():
    """
    Defaults from disk, ignoring anything unknown or unparseable.

    What was set in the console wins over the command line, always. The
    alternative - command line wins - means every restart silently undoes an
    afternoon of listening, and the flag that caused it is in a shortcut
    nobody reads. --reset-config is there for when the file is the problem.
    """
    global DEFAULTS, SAVED_REFERENCE, SAVED_LANGUAGE, SAVED_SPEAKER
    DEFAULTS = dict(KNOB_DEFAULTS)
    SAVED_REFERENCE = None
    SAVED_LANGUAGE = None
    SAVED_SPEAKER = None
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
    # Same rule as the model and the reference: chosen in the console, kept
    # across restarts. Held aside rather than put in DEFAULTS because these
    # name a language and a voice, not a sampling value, and DEFAULTS is
    # handed straight to the model as keyword arguments.
    if saved.get("language") in SUPPORTED_LANGUAGES:
        SAVED_LANGUAGE = str(saved["language"])
    if saved.get("speaker"):
        SAVED_SPEAKER = str(saved["speaker"])
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
        body["language"] = CFG.language
        body["speaker"] = getattr(CFG, "speaker", "")
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


# Three families, and they are asked for speech in three different ways.
#
# Only the cloning one was ever wired up here, which is why the CustomVoice and
# VoiceDesign entries in the model list were half a feature: you could load one
# and then had no way to say WHICH built-in voice, or to give the description
# that is the entire point of VoiceDesign. The reference recording is ignored by
# both - they do not clone anything.
def model_kind(model_id=None):
    mid = (model_id or CFG.model_id).lower()
    if "customvoice" in mid:
        return "custom"
    if "voicedesign" in mid:
        return "design"
    return "clone"


def model_speakers():
    """The built-in voices of the loaded model, or [] if it has none."""
    if _model is None:
        return []
    try:
        names = _model.get_supported_speakers()
    except Exception:
        return []
    return sorted(str(n) for n in (names or []))


def _speaker(wanted=None):
    """The built-in voice for this call, or the saved one, or whatever exists."""
    wanted = (wanted or getattr(CFG, "speaker", "") or "").strip()
    names = model_speakers()
    if wanted and (not names or wanted.lower() in {n.lower() for n in names}):
        return wanted
    return names[0] if names else wanted


def synth_once(text: str, instruct: str = "", speaker: str = "",
               xvec: bool = False, **knobs):
    """one attempt, whole answer at once; returns (audio, sample_rate)"""
    global _sample_rate
    m = model()
    kind = model_kind()

    if kind == "custom":
        wavs, sr = m.generate_custom_voice(
            text=text, speaker=_speaker(speaker), language=CFG.language,
            instruct=instruct or None, **knobs)
    elif kind == "design":
        if not instruct:
            raise RuntimeError(
                "VoiceDesign строит голос по описанию - без инструкции ему нечего делать")
        wavs, sr = m.generate_voice_design(
            text=text, instruct=instruct, language=CFG.language, **knobs)
    else:
        wavs, sr = m.generate_voice_clone(
            text=text, language=CFG.language,
            ref_audio=str(_slice_path),
            # x-vector only takes the timbre and skips the transcript entirely.
            # Worse than the full path, and the reason it is offered: without it
            # a new reference cannot be used until somebody writes down exactly
            # what it says, and whisper is not installed here to do that.
            ref_text="" if xvec else _prompt_text,
            x_vector_only_mode=xvec,
            instruct=instruct or None, **knobs,
        )

    _sample_rate = sr
    audio = wavs[0] if isinstance(wavs, (list, tuple)) else wavs
    if hasattr(audio, "detach"):
        audio = audio.detach().cpu().numpy()
    return np.asarray(audio, dtype="float32").squeeze(), sr


def synth_stream(text: str, instruct: str = "", chunk_size: int = None,
                 speaker: str = "", xvec: bool = False, **knobs):
    """yields (audio_chunk, sample_rate) as the model produces them"""
    global _sample_rate
    m = model()
    kind = model_kind()
    chunk = chunk_size or DEFAULTS.get("chunk_size", CFG.chunk_size)

    if kind == "custom":
        stream = m.generate_custom_voice_streaming(
            text=text, speaker=_speaker(speaker), language=CFG.language,
            chunk_size=chunk, instruct=instruct or None, **knobs)
    elif kind == "design":
        if not instruct:
            raise RuntimeError(
                "VoiceDesign строит голос по описанию - без инструкции ему нечего делать")
        stream = m.generate_voice_design_streaming(
            text=text, instruct=instruct, language=CFG.language,
            chunk_size=chunk, **knobs)
    else:
        stream = m.generate_voice_clone_streaming(
            text=text, language=CFG.language,
            ref_audio=str(_slice_path),
            ref_text="" if xvec else _prompt_text, xvec_only=xvec,
            chunk_size=chunk, instruct=instruct or None, **knobs)

    for chunk_audio, sr, _timing in stream:
        _sample_rate = sr
        a = np.asarray(chunk_audio, dtype="float32").squeeze()
        if a.size:
            yield a, sr


def synth_checked(text: str, tries: int = 3, instruct: str = "",
                  keep_bad: bool = False, speaker: str = "",
                  xvec: bool = False, **knobs):
    """
    Synthesis with the take checked and retaken if it is no good.

    Same contract as the CosyVoice sidecar: three bad takes and the answer
    stops rather than being skipped, because a silently missing sentence still
    sounds like a complete answer.
    """
    last = "no attempt"
    for attempt in range(1, tries + 1):
        try:
            audio, sr = synth_once(text, instruct, speaker=speaker, xvec=xvec, **knobs)
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
        "language": CFG.language,
        "languages": SUPPORTED_LANGUAGES,
        "loading": {
            "busy": LOAD["busy"],
            "phase": LOAD["phase"],
            "model": LOAD["model"],
            "elapsed": round(time.time() - LOAD["started"], 1) if LOAD["busy"] else 0,
            "mb": round(LOAD["bytes"] / 2**20) if LOAD["busy"] else 0,
            "error": LOAD["error"],
            "took": LOAD["took"],
        },
        "kind": model_kind(),
        "speaker": getattr(CFG, "speaker", ""),
        "speakers": model_speakers(),
        "vram": vram(),
    }


@app.post("/speak")
def speak(body: dict = Body(...)):
    text = (body.get("text") or "").strip()
    if not text:
        return JSONResponse({"error": "text is empty"}, status_code=400)
    mode = (body.get("mode") or "stream").lower()
    instruct = (body.get("instruct") or "").strip()
    # named per request so the console can try a voice without saving it, and
    # so Jarvis keeps getting whatever was saved
    speaker = (body.get("speaker") or "").strip()
    xvec = bool(body.get("xvec", False))
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
                    for audio, sr in synth_stream(text, instruct, chunk_size=chunk,
                                                  speaker=speaker, xvec=xvec, **knobs):
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
                    audio, sr = synth_checked(text, instruct=instruct,
                                              speaker=speaker, xvec=xvec, **knobs)
                except RuntimeError as e:
                    print(f"giving up on {text[:60]!r}: {e}", flush=True)
                    return
                yield frame(wav_bytes(trim_head(audio, sr), sr), FLAG_FELL_BACK)
                yield END
                return

            try:
                audio, sr = synth_checked(
                    text, tries=1 if keep_bad else 3, instruct=instruct,
                    keep_bad=keep_bad, speaker=speaker, xvec=xvec, **knobs)
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
            "language": CFG.language,
            "speaker": getattr(CFG, "speaker", ""),
            "kind": model_kind(),
            "speakers": model_speakers(),
            "languages": SUPPORTED_LANGUAGES,
            "saved_to": str(CONFIG_PATH)}


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
    # Not a sampling knob, so it is handled apart from the loop above: it
    # names a voice or a language rather than a number.
    if "language" in body:
        wanted = str(body["language"]).strip() or "Russian"
        if wanted not in SUPPORTED_LANGUAGES:
            return JSONResponse(
                {"error": "язык {!r} не из списка: {}".format(
                    wanted, ", ".join(SUPPORTED_LANGUAGES))}, status_code=400)
        CFG.language = wanted
        changed["language"] = wanted
    if "speaker" in body:
        CFG.speaker = str(body["speaker"]).strip()
        changed["speaker"] = CFG.speaker

    save_config()
    print("defaults changed: " + ", ".join(f"{k}={v}" for k, v in changed.items()),
          flush=True)
    return {"defaults": DEFAULTS, "changed": changed,
            "language": CFG.language, "speaker": getattr(CFG, "speaker", "")}


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
        out = HERE / "out" / "_sidecar_qwen_ref.wav"
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
    """
    What can be loaded, and which of them is already on this disk.

    The size matters to the person choosing: picking one that is not here yet
    means several gigabytes off the network before anything is heard, and the
    old list gave no hint which was which.
    """
    out = []
    for m in KNOWN_MODELS:
        mb = round(_cache_bytes(m["id"]) / 2**20)
        out.append(dict(m, cached_mb=mb, on_disk=mb > 100))
    return {"current": CFG.model_id, "known": out}


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

    if LOAD["busy"]:
        return JSONResponse({"error": "модель уже грузится"}, status_code=409)

    previous = CFG.model_id

    def run():
        with _lock:
            unload()
            CFG.model_id = wanted
            try:
                model()
            except Exception as e:
                CFG.model_id = previous
                LOAD.update(busy=False, phase="",
                            error=f"не загрузилась {wanted}: {e}")
                print(f"could not load {wanted}: {e}", flush=True)
                return
            save_config()

            # warm the path that will actually be used, same reason as at
            # startup - but only for the cloning models, since it is the
            # cloning path that carries the cost
            if model_kind() == "clone":
                LOAD["busy"], LOAD["phase"] = True, "прогреваю голос"
                try:
                    for _ in synth_stream("Готово."):
                        pass
                except Exception as e:
                    print(f"warm-up after the swap failed: {e}", flush=True)
                LOAD["busy"], LOAD["phase"] = False, ""
            print(f"model is now {wanted}", flush=True)

    # Answered at once, followed through /health. A model swap is a fresh load
    # of several gigabytes; holding the request open for it is how the console
    # ended up with nothing to show.
    threading.Thread(target=run, daemon=True).start()
    return {"model": wanted, "started": True}


@app.get("/voices")
def voices():
    """Every voice pack on disk, and which one the Заготовки tab is editing."""
    return {"current": PACK_ID, "packs": list_packs(),
            "dir": str(VOICES_DIR)}


@app.post("/voices/select")
def voices_select(payload: dict = Body(...)):
    """Work on a different voice."""
    global PACK_ID
    wanted = (payload.get("id") or "").strip()
    if not (VOICES_DIR / wanted / "voice.toml").exists():
        return JSONResponse({"error": "нет такого голоса"}, status_code=404)
    PACK_ID = wanted
    return {"current": PACK_ID}


@app.post("/voices/new")
def voices_new(payload: dict = Body(...)):
    """
    Start a new voice, copying the shape of an existing one.

    Only the shape: the reactions a pack must answer to and the lines each clip
    says. No recordings - those come from the record button, in whatever voice
    the reference is set to now. That separation is the point of the whole
    thing: the words stay the same and the voice changes.
    """
    import tomlkit

    global PACK_ID

    folder_name = (payload.get("id") or "").strip().lower()
    if not folder_name or not all(c.isalnum() or c in "-_" for c in folder_name):
        return JSONResponse(
            {"error": "имя из латиницы, цифр, дефиса и подчёркивания"}, status_code=400)
    if len(folder_name) > 48:
        return JSONResponse({"error": "имя слишком длинное"}, status_code=400)

    target = VOICES_DIR / folder_name
    if target.exists():
        return JSONResponse({"error": f"голос {folder_name} уже есть"}, status_code=409)

    source = VOICES_DIR / (payload.get("from") or PACK_ID)
    if not (source / "voice.toml").exists():
        return JSONResponse({"error": "не найден голос-образец"}, status_code=404)

    doc = tomlkit.parse((source / "voice.toml").read_text(encoding="utf-8"))
    doc["voice"]["id"] = folder_name
    doc["voice"]["name"] = (payload.get("name") or folder_name).strip()
    doc["voice"]["author"] = (payload.get("author") or "").strip()

    target.mkdir(parents=True)
    for lang in _pack_langs(doc):
        (target / lang).mkdir(exist_ok=True)
    (target / "voice.toml").write_text(tomlkit.dumps(doc), encoding="utf-8")

    PACK_ID = folder_name
    print(f"[pack] new voice {folder_name} from {source.name}", flush=True)
    return {"created": folder_name, "current": PACK_ID,
            "from": source.name, "packs": list_packs()}


@app.post("/voices/delete")
def voices_delete(payload: dict = Body(...)):
    """
    Remove a voice, but never one that ships with the assistant.

    The four that come with it are in git; a console that could delete them
    would be a console that can break the install with one click.
    """
    import shutil
    global PACK_ID
    SHIPPED = {"jarvis-og", "jarvis-og-tts", "jarvis-remaster", "jarvis-howdy"}
    wanted = (payload.get("id") or "").strip()
    if wanted in SHIPPED:
        return JSONResponse(
            {"error": f"{wanted} поставляется с ассистентом, удалять нельзя"},
            status_code=403)
    target = VOICES_DIR / wanted
    if not (target / "voice.toml").exists():
        return JSONResponse({"error": "нет такого голоса"}, status_code=404)
    shutil.rmtree(target)
    if PACK_ID == wanted:
        PACK_ID = "jarvis-og-tts"
    print(f"[pack] deleted voice {wanted}", flush=True)
    return {"deleted": wanted, "current": PACK_ID, "packs": list_packs()}


@app.get("/pack")
def pack():
    """The voice pack: what is in it, what each clip says, how long it runs."""
    doc, _ = _pack_doc()
    meta = doc.get("voice", {}) or {}
    return {
        "id": str(meta.get("id", "")),
        "name": str(meta.get("name", "")),
        "dir": str(_pack_dir()),
        "packs": list_packs(),
        "lang": PACK_LANG,
        "rows": pack_rows(),
        "reference": Path(str(CFG.reference)).name,
        "slice": {"start": CFG.start, "length": CFG.length,
                  "secs": round(_slice_secs, 2)},
        "model": CFG.model_id,
        "loaded": _model is not None,
        "kind": model_kind(),
        "can_bake": model_kind() == "clone",
    }


@app.get("/pack/audio")
def pack_audio(stem: str):
    """One clip, for the console to play."""
    wav = _clip_path(stem)
    if wav is None or not wav.exists():
        return JSONResponse({"error": "нет такого клипа"}, status_code=404)
    return Response(content=wav.read_bytes(), media_type="audio/wav")


@app.post("/pack/line")
def pack_line(payload: dict = Body(...)):
    """Change what a clip is supposed to say, leaving the rest of the file alone."""
    import tomlkit
    stem = (payload.get("stem") or "").strip()
    if _clip_path(stem) is None:
        return JSONResponse({"error": "недопустимое имя клипа"}, status_code=400)
    text = (payload.get("text") or "").strip()

    doc, path = _pack_doc()
    _set_line(doc, stem, text)
    path.write_text(tomlkit.dumps(doc), encoding="utf-8")
    return {"stem": stem, "text": text}


@app.post("/pack/bake")
def pack_bake(payload: dict = Body(...)):
    """
    Record one clip again, in the voice currently loaded.

    The take is checked the same way every spoken answer is - bad_take() rejects
    a mangled one and it is retaken - so this needs no whisper, which is why the
    baking script could not run in this environment at all. What it does NOT do
    is the script's transcribe-and-compare: a person is listening here, and the
    play button is right beside this one.

    The previous file is kept before it is overwritten. The model never produces
    the same take twice, so a good clip replaced by a worse one cannot be got
    back by asking again.
    """
    import tomlkit

    stem = (payload.get("stem") or "").strip()
    wav_path = _clip_path(stem)
    if wav_path is None:
        return JSONResponse({"error": "недопустимое имя клипа"}, status_code=400)

    doc, doc_path = _pack_doc()
    lines = _pack_lines(doc)
    sent = (payload.get("text") or "").strip()
    text = sent or str(lines.get(stem, "")).strip()
    if not text:
        return JSONResponse(
            {"error": "нечего произносить: у клипа нет текста"}, status_code=400)

    # a text sent along with the bake is also the clip's new text
    if sent and str(lines.get(stem, "")) != sent:
        _set_line(doc, stem, sent)
        doc_path.write_text(tomlkit.dumps(doc), encoding="utf-8")

    # Said before the model is asked, not after it fails three times.
    #
    # These clips ARE the assistant's voice, and only the cloning models make a
    # voice from the reference recording. Loading VoiceDesign and pressing
    # record used to spend three attempts and half a minute to arrive at the
    # same answer.
    if model_kind() != "clone":
        return JSONResponse(
            {"error": "Заготовки начитываются клонирующей моделью — загруженная "
                      "{} голос по образцу не делает. Выберите -Base на вкладке "
                      "«Синтез».".format(CFG.model_id.split("/")[-1])},
            status_code=409)

    knobs = knobs_from(payload)
    t0 = time.time()
    try:
        with _lock:
            _stats["requests"] += 1
            audio, sr = synth_checked(text, tries=3, **knobs)
    except RuntimeError as e:
        return JSONResponse({"error": str(e)}, status_code=503)
    except Exception as e:
        return JSONResponse({"error": f"{type(e).__name__}: {e}"}, status_code=500)

    data = wav_bytes(trim_head(audio, sr), sr)

    kept = None
    if wav_path.exists():
        spare = VOICES_DIR / (PACK_ID + "-previous") / PACK_LANG
        spare.mkdir(parents=True, exist_ok=True)
        kept = spare / (stem + ".wav")
        kept.write_bytes(wav_path.read_bytes())

    wav_path.parent.mkdir(parents=True, exist_ok=True)
    wav_path.write_bytes(data)

    print(f"[pack] baked {stem} in {time.time()-t0:.1f}s :: {text}", flush=True)
    return {
        "stem": stem, "text": text,
        "secs": _clip_seconds(wav_path),
        "took_secs": round(time.time() - t0, 1),
        "kept_previous": bool(kept),
    }


@app.get("/gpu")
def gpu():
    """The card, and who is holding it."""
    rows = gpu_processes()
    return {
        "vram": vram(),
        "processes": rows,
        "can_kill": BOUND_HOST in LOOPBACK,
        "supported": os.name == "nt",
        "ours": os.getpid(),
    }


@app.post("/gpu/kill")
def gpu_kill(payload: dict = Body(...)):
    """
    End one process that is holding the card.

    Deliberately narrow. It refuses anything not currently in the GPU list, so
    it cannot be used as a general process killer, and it refuses everything
    when this server is not on the loopback - a console that ends processes is
    for the person at this keyboard, not for the network.
    """
    import psutil

    if BOUND_HOST not in LOOPBACK:
        return JSONResponse(
            {"error": "Сайдкар слушает {}, а не петлю. Завершение процессов "
                      "отсюда выключено.".format(BOUND_HOST)},
            status_code=403,
        )

    try:
        pid = int(payload.get("pid"))
    except (TypeError, ValueError):
        return JSONResponse({"error": "нужен pid"}, status_code=400)

    rows = {r["pid"]: r for r in gpu_processes()}
    row = rows.get(pid)
    if row is None:
        return JSONResponse(
            {"error": "Процесс {} не держит видеопамять. Завершать можно "
                      "только то, что есть в этом списке.".format(pid)},
            status_code=404,
        )

    refusal = may_kill(row)
    if refusal:
        return JSONResponse({"error": refusal}, status_code=403)

    try:
        p = psutil.Process(pid)
        p.terminate()
        try:
            p.wait(timeout=4)
        except psutil.TimeoutExpired:
            # asked politely, twice
            p.kill()
            p.wait(timeout=4)
    except psutil.NoSuchProcess:
        pass
    except psutil.AccessDenied:
        return JSONResponse(
            {"error": "Нет прав завершить {} (pid {}). Он запущен от другого "
                      "пользователя или от администратора.".format(row["name"], pid)},
            status_code=403,
        )
    except Exception as e:
        return JSONResponse({"error": str(e)}, status_code=500)

    print("[gpu] ended {} (pid {}), it held {} MB".format(row["name"], pid, row["mb"]),
          flush=True)
    return {"ended": row["name"], "pid": pid, "freed_mb": row["mb"]}


@app.post("/unload")
def api_unload():
    """Hand the video memory back. The next /speak loads the model again."""
    with _lock:
        was = unload()
    return {"unloaded": was, "vram": vram()}


@app.post("/reload")
def api_reload():
    """
    Start loading, and answer at once.

    It used to block for the whole load - the better part of a minute, or
    several minutes the first time a model is fetched - so the page had a dead
    request in flight and nothing to show. The work happens on a thread now and
    the console follows it through /health.
    """
    if LOAD["busy"]:
        return {"started": False, "already": True, "phase": LOAD["phase"]}
    if _model is not None:
        return {"started": False, "loaded": True, "vram": vram()}

    def run():
        try:
            with _lock:
                model()
        except Exception as e:
            print(f"load failed: {e}", flush=True)

    threading.Thread(target=run, daemon=True).start()
    return {"started": True}


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
.progress{height:4px;background:var(--line);border-radius:2px;overflow:hidden;
  margin-top:11px;display:none}
.progress.on{display:block}
.progress i{display:block;height:100%;width:35%;background:var(--acc);border-radius:2px;
  animation:slide 1.1s ease-in-out infinite}
@keyframes slide{0%{margin-left:-35%}100%{margin-left:100%}}
@media (prefers-reduced-motion: reduce){.progress i{animation:none;width:100%;opacity:.5}}
.tabs{display:flex;gap:6px;margin-bottom:16px;border-bottom:1px solid var(--line)}
.tab{background:transparent;color:var(--soft);border:0;border-bottom:2px solid transparent;
  border-radius:0;padding:8px 14px;font:600 13.5px system-ui;cursor:pointer}
.tab.on{color:var(--acc);border-bottom-color:var(--acc)}
/* fixed layout, or the paths in the first column push the buttons out of
   the card - they are long enough to do it on any real machine */
table{width:100%;table-layout:fixed;border-collapse:collapse;font-variant-numeric:tabular-nums}
th{text-align:left;font:600 11.5px system-ui;text-transform:uppercase;letter-spacing:.07em;
  color:var(--soft);padding:0 9px 8px 0;border-bottom:1px solid var(--line);
  cursor:pointer;user-select:none;white-space:nowrap}
th:hover{color:var(--ink)}
th.num,td.num{text-align:right;padding-right:14px}
th.act{cursor:default;text-align:right;padding-right:0}
th.act:hover{color:var(--soft)}
td{padding:9px 9px 9px 0;border-bottom:1px solid var(--line);font-size:13.5px;
  vertical-align:middle;overflow:hidden}
td.act{text-align:right;padding-right:0;white-space:nowrap}
.pname{white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
tr.blocked td{color:var(--soft)}
tr.mine td{color:var(--acc)}
button.small{padding:5px 11px;font-size:12.5px}
button.danger{background:transparent;color:var(--warn);border:1px solid var(--line)}
button.danger:hover{border-color:var(--warn)}
.why{font-size:12px;color:var(--soft);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.arrow{color:var(--acc);font-size:10px}
</style></head><body><div class="wrap">
<h1>Jarvis TTS</h1>
<div class="sub" id="banner">Qwen3-TTS. Модель держится загруженной, одна генерация за раз.</div>

<div class="tabs">
  <button class="tab on" id="tab-tts" onclick="showTab('tts')">Синтез</button>
  <button class="tab" id="tab-pack" onclick="showTab('pack')">Голоса</button>
  <button class="tab" id="tab-gpu" onclick="showTab('gpu')">Видеопамять</button>
</div>

<div id="pane-tts">

<div class="card">
  <div class="tabs" style="margin-bottom:0;border-bottom:0">
    <button class="tab on" id="mode-clone" onclick="setMode('clone')">Клонирование</button>
    <button class="tab" id="mode-custom" onclick="setMode('custom')">Готовые голоса</button>
    <button class="tab" id="mode-design" onclick="setMode('design')">Голос по описанию</button>
  </div>
  <div class="meta" id="modenote" style="margin-top:11px"></div>
  <div class="row" style="margin-top:11px">
    <div><label>Размер модели</label><select id="modelsize" onchange="modeModelNote()"></select></div>
    <div><label>Язык</label><select id="language" onchange="saveVoice()"></select></div>
    <div id="speakerbox"><label>Встроенный голос</label>
      <select id="speaker" onchange="saveVoice()"></select></div>
  </div>
  <div class="bar">
    <button class="ghost" id="modeload" onclick="modeLoad()">Загрузить эту модель</button>
    <button class="ghost" id="modeunload" onclick="adm('/unload')">Выгрузить из памяти</button>
    <span id="modemodel" class="meta"></span>
  </div>
  <div class="progress" id="loadbar"><i></i></div>
  <div class="meta" id="loadnote" style="margin-top:7px"></div>
  <div class="meta" id="model_note" style="margin-top:5px"></div>
</div>

<div class="card" id="refcard">
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
  <div style="margin-top:10px"><label style="display:inline">
    <input type="checkbox" id="xvec" style="width:auto;margin-right:7px">
    только x-vector — расшифровка отрезка не нужна</label></div>
  <div class="meta" style="margin-top:5px">Берётся только тембр, без разбора речи.
    Качество ниже, зато новый эталон можно пробовать сразу, не записывая вручную,
    что в нём сказано — whisper в это окружение намеренно не ставился.</div>
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

<div class="card" id="designcard" style="display:none">
  <label>Описание голоса</label>
  <textarea id="design" placeholder="Например: низкий спокойный мужской голос, говорит медленно и уверенно"></textarea>
  <div class="meta" style="margin-top:6px">Это <b>весь</b> вход VoiceDesign: голос строится
    из описания, образец записи не используется. Без описания она отказывается говорить.</div>
</div>


<div class="card">
  <label>Текст</label>
  <textarea id="text">Система работает штатно, сэр. Все процессы в пределах нормы.</textarea>
  <div style="margin-top:11px" id="instructbox">
    <label>Инструкция — как говорить</label>
    <input id="instruct" placeholder="пусто = обычное клонирование по образцу">
    <div class="meta warn" id="instructnote" style="margin-top:5px">Модель часто зачитывает инструкцию вслух вместо того,
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
  <div class="meta" id="langnote" style="margin-top:8px"></div>

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

<select id="model_id" style="display:none"></select>



<div class="card"><div id="list"></div></div>
</div><!-- /pane-tts -->

<div id="pane-pack" style="display:none">
  <div class="card">
    <div class="row" style="margin-top:0">
      <div style="flex:2"><label>Голос</label>
        <select id="packpick" onchange="packSelect()"></select></div>
      <div style="flex:1"><label>&nbsp;</label>
        <button class="ghost" style="width:100%" onclick="newVoice()">Новый голос</button></div>
      <div style="flex:1"><label>&nbsp;</label>
        <button class="ghost" style="width:100%" onclick="dropVoice()">Удалить</button></div>
    </div>
    <div class="meta" id="packlist" style="margin-top:10px"></div>
    <div class="meta" style="margin-top:9px">Новый голос заводится <b>по образцу существующего</b>:
      берутся те же реакции и те же реплики, но ни одной записи — их надо начитать
      кнопками ниже, тем эталоном, который стоит сейчас на вкладке «Синтез».
      Так слова остаются прежними, а голос меняется. Появится в настройках Джарвиса
      наравне с остальными.</div>
  </div>

  <div class="card">
    <div class="bar" style="margin-top:0">
      <button class="ghost" onclick="packLoad()">Обновить</button>
      <button class="ghost" id="bakeall" onclick="bakeAll()">Перегенерировать всё</button>
      <span id="packmeta" class="meta"></span>
    </div>
    <div class="meta" style="margin-top:9px">Это реплики, которые Джарвис проигрывает
      <b>не спрашивая модель</b>: «да, сэр» на своё имя, «думаю над ответом» пока идёт ответ.
      Они записаны тем же эталоном, которым клонируется живой голос, — <b>сменил эталон,
      значит заготовки больше ему не соответствуют</b> и их стоит перезаписать.
      Текст можно поправить прямо в строке. Прежний файл сохраняется рядом с пакетом
      перед перезаписью: одинаковый дубль модель дважды не выдаёт.</div>
    <div id="packstatus" class="meta" style="margin-top:9px"></div>
  </div>

  <div class="card">
    <table>
      <colgroup><col style="width:13%"><col style="width:13%"><col style="width:41%">
                <col style="width:9%"><col style="width:24%"></colgroup>
      <thead><tr>
        <th onclick="packSort('reaction')">Реакция <span id="pr-reaction" class="arrow"></span></th>
        <th onclick="packSort('stem')">Файл <span id="pr-stem" class="arrow"></span></th>
        <th>Текст</th>
        <th class="num" onclick="packSort('secs')">Длит. <span id="pr-secs" class="arrow"></span></th>
        <th class="act"></th>
      </tr></thead>
      <tbody id="packrows"><tr><td colspan="5" class="meta">…</td></tr></tbody>
    </table>
  </div>
</div><!-- /pane-pack -->

<div id="pane-gpu" style="display:none">
  <div class="card">
    <div class="bar" style="margin-top:0">
      <button class="ghost" onclick="adm('/unload')">Освободить карту</button>
      <span id="vram" class="meta"></span>
    </div>
    <div class="meta" style="margin-top:9px">Наша собственная модель — обычно самое
      крупное, что занимает карту, и завершить сайдкар в списке ниже нельзя. Выгрузка —
      единственный способ отдать её память, и она не останавливает сайдкар: следующий
      запрос на синтез загрузит модель заново.
      <b>Загружается</b> модель на вкладке «Синтез», рядом с выбором режима — здесь её
      только освобождают.</div>
  </div>

  <div class="card">
    <div class="bar" style="margin-top:0">
      <button class="ghost" onclick="gpuLoad()">Обновить</button>
      <span id="gpucard" class="meta"></span>
    </div>
    <div class="meta" style="margin-top:9px">Цифры по процессам берутся из счётчиков Windows —
      тех же, что показывает диспетчер задач. <b>Они не складываются в объём карты</b>: композитор
      рабочего стола <code>dwm</code> держит поверхности за все окна сразу, поэтому одна и та же
      память попадает и в его строку, и в строку приложения. Объём карты выше читается отдельно,
      у драйвера.</div>
  </div>

  <div class="card">
    <table>
      <colgroup><col style="width:56%"><col style="width:11%">
                <col style="width:17%"><col style="width:16%"></colgroup>
      <thead><tr>
        <th onclick="sortBy('name')">Процесс <span id="ar-name" class="arrow"></span></th>
        <th class="num" onclick="sortBy('pid')">PID <span id="ar-pid" class="arrow"></span></th>
        <th class="num" onclick="sortBy('mb')">Видеопамять <span id="ar-mb" class="arrow"></span></th>
        <th class="act"></th>
      </tr></thead>
      <tbody id="gpurows"><tr><td colspan="4" class="meta">…</td></tr></tbody>
    </table>
    <div id="gpustatus" class="meta" style="margin-top:11px"></div>
  </div>
</div><!-- /pane-gpu -->
<script>
const $=id=>document.getElementById(id)
// What kind of model is loaded decides which controls make sense: the
// cloning ones take their voice from the reference recording and ignore both
// of these, CustomVoice needs a name, VoiceDesign needs a description and
// nothing else.
let KIND='clone'
// Which of the three the person is working in. The model follows the mode
// rather than the other way round: picking "Готовые голоса" and then having to
// know that this means a CustomVoice checkpoint is the kind of thing only the
// person who wrote it remembers.
let MODE='clone'

const MODE_MODELS = {
  clone:  {sizes:['0.6B','1.7B'], suffix:'Base',
           note:'Голос клонируется с образца записи внизу. Это то, чем говорит Джарвис.'},
  custom: {sizes:['0.6B','1.7B'], suffix:'CustomVoice',
           note:'Девять встроенных голосов. Образец записи не используется, '
                +'инструкция управляет манерой.'},
  design: {sizes:['1.7B'], suffix:'VoiceDesign',
           note:'Голос строится из описания словами. Ни образец, ни встроенные '
                +'голоса не участвуют.'},
}

function modeModelId(){
  const m = MODE_MODELS[MODE]
  const size = $('modelsize').value || m.sizes[m.sizes.length-1]
  return 'Qwen/Qwen3-TTS-12Hz-' + size + '-' + m.suffix
}

function modeModelNote(){
  const id = modeModelId()
  const known = MODELS.find(x => x.id === id)
  const here = known ? (known.on_disk ? 'на диске' : 'надо скачать') : ''
  const inMemory = (CURRENT_MODEL === id) && MODEL_IN_MEMORY
  $('modemodel').textContent = id.split('/').pop() + (here ? ' ' + DOT + ' ' + here : '')
    + (inMemory ? ' ' + DOT + ' в памяти' : '')
  // only a model that is genuinely loaded has nothing left to do
  $('modeload').disabled = inMemory || BUSY
  // nothing to hand back when nothing is holding anything
  $('modeunload').disabled = !MODEL_IN_MEMORY || BUSY
  showModelNote()
}

function setMode(which){
  MODE = which
  for(const name of ['clone','custom','design']){
    $('mode-'+name).className = (name===which) ? 'tab on' : 'tab'
  }
  const m = MODE_MODELS[which]
  $('modenote').textContent = m.note
  // Prefer the size already in memory, then whatever was picked before, then
  // the larger one. Landing on 0.6B by default would quietly answer in a
  // different voice than the one the assistant actually speaks with.
  const sel = $('modelsize')
  const loaded = (CURRENT_MODEL.match(/12Hz-([0-9.]+B)-/) || [])[1]
  const keep = sel.value
  sel.innerHTML = m.sizes.map(s => '<option value="'+s+'">'+s+'</option>').join('')
  for(const want of [loaded, keep, m.sizes[m.sizes.length-1]]){
    if(want && m.sizes.indexOf(want) >= 0){ sel.value = want; break }
  }
  $('speakerbox').style.display = (which==='custom') ? '' : 'none'
  $('designcard').style.display = (which==='design') ? '' : 'none'
  $('refcard').style.display = (which==='clone') ? '' : 'none'
  // Only the built-in voices actually follow an instruction. The cloning
  // models read it out loud instead - measured on Russian, English and
  // Chinese - and VoiceDesign has its own box above, which is not the same
  // thing. A field that does the opposite of its label is worse than no field.
  $('instructbox').style.display = (which==='custom') ? '' : 'none'
  modeModelNote()
}

async function modeLoad(){
  const id = modeModelId()
  if(![...$('model_id').options].some(o => o.value === id)){
    $('modemodel').textContent = 'сайдкар не знает такой модели: ' + id
    return
  }
  $('model_id').value = id
  await applyModel(true)
}

function applyKind(h){
  KIND = h.kind || 'clone'
  const names = h.speakers || []
  const sel = $('speaker')

  // Shown always, even when it cannot be used.
  //
  // It used to hide itself whenever the loaded model had no built-in voices,
  // which is most of the time - and a control that is simply absent is
  // indistinguishable from one that was never built. Better to stand there
  // greyed out and say which model would give it something to offer.
  if(KIND==='custom' && names.length){
    sel.disabled = false
    if(sel.options.length !== names.length || sel.options[0].disabled){
      sel.innerHTML = names.map(n => '<option value="'+esc(n)+'">'+esc(n)+'</option>').join('')
    }
    if(h.speaker) sel.value = h.speaker
  }else{
    sel.disabled = true
    sel.innerHTML = '<option disabled selected>' +
      (KIND==='design' ? 'VoiceDesign строит голос по описанию'
                       : 'эта модель клонирует голос с образца')
      + '</option>'
  }
  const note = $('instructnote')
  note.className = 'meta'
  note.textContent = 'Управляет манерой встроенного голоса: «говори бодро», «медленно и веско».'
    + ' Клонирующие модели её не выполняют, а зачитывают вслух, поэтому там этого поля нет.'
  const lang = ($('language').value === 'Auto')
    ? 'Язык определяется по тексту — полезно, когда в ответе попадаются английские слова. '
    : 'Текст читается как выбранный язык, что бы в нём ни было написано. '
  const voice = (KIND==='custom')
    ? 'Голос выбирается из девяти встроенных.'
    : (KIND==='design')
      ? 'Голос задаётся описанием в поле инструкции ниже.'
      : 'Встроенных голосов у этой модели нет — она клонирует голос с образца записи. '
        + 'Девять готовых голосов появятся, если загрузить ниже модель CustomVoice.'
  $('langnote').textContent = lang + voice
}

async function saveVoice(){
  const body = {language: $('language').value}
  if(KIND==='custom' && $('speaker').value) body.speaker = $('speaker').value
  try{
    const r = await fetch('/config',{method:'POST',headers:{'Content-Type':'application/json'},
      body:JSON.stringify(body)})
    const j = await r.json()
    if(!r.ok){ $('langnote').textContent = j.error || ('ошибка '+r.status); return }
  }catch(e){ $('langnote').textContent = 'сайдкар не отвечает'; return }
  refresh()
}

async function refresh(){
  try{
    const h=await (await fetch('/health')).json()
    const v=h.vram||{}
    $('vram').textContent=(h.ok?'в памяти':'выгружена')
      +(v.card_free_mb?(' · держим '+v.ours_held_mb+' МБ, на карте свободно '+v.card_free_mb+' из '+v.card_total_mb):'')
    if(!$('language').options.length && h.languages){
      $('language').innerHTML = h.languages.map(l =>
        '<option value="'+esc(l)+'">'+esc(l)+'</option>').join('')
    }
    if(h.language) $('language').value = h.language
    CURRENT_MODEL = h.model || ''
    MODEL_IN_MEMORY = !!h.ok
    applyKind(h)
    modeModelNote()
    showLoad(h.loading)
    $('banner').textContent=(h.ok?'':'модель не в памяти \u00b7 ')
      +h.model+' \u00b7 эталон '+h.slice.secs+'s \u00b7 '+(h.sample_rate||'?')+' Гц'
  }catch(e){ $('vram').textContent='сайдкар не отвечает' }
}
// While a model is loading the page follows it once a second; the rest of the
// time five seconds is plenty and the counters are not free.
let FAST=null, BUSY=false, WASBUSY=false
function watchLoad(on){
  if(on && !FAST){ FAST = setInterval(refresh, 1000) }
  if(!on && FAST){ clearInterval(FAST); FAST = null }
}

function showLoad(l){
  const bar = $('loadbar'), note = $('loadnote')
  WASBUSY = BUSY
  BUSY = !!(l && l.busy)
  if(l && l.busy){
    bar.className = 'progress on'
    // only while it is arriving: once the weights are on disk their size is
    // not news, and the same number under "читаю веса" reads like a download
    const size = (l.mb && l.phase.indexOf('скач') === 0) ? (' \u00b7 ' + (l.mb >= 1024
      ? (l.mb/1024).toFixed(1) + ' ГБ на диске' : l.mb + ' МБ на диске')) : ''
    note.className = 'meta'
    note.textContent = l.phase + ' \u00b7 ' + Math.round(l.elapsed) + ' с' + size
      + (l.model ? ' \u00b7 ' + l.model.split('/').pop() : '')
    watchLoad(true)
    return
  }
  bar.className = 'progress'
  watchLoad(false)
  if(l && l.error){ note.className = 'meta warn'; note.textContent = l.error }
  else if(l && l.took){ note.className = 'meta'
    note.textContent = 'загружено за ' + l.took + ' с'
    if(WASBUSY) loadModels() }
  else { note.textContent = '' }
}

async function adm(path){
  $('loadnote').className = 'meta'
  $('loadnote').textContent = 'запрашиваю...'
  try{ await fetch(path,{method:'POST'}) } finally { refresh() }
}
function body(){
  // The instruction field means different things per mode, so each mode sends
  // what it actually has: a style hint for the built-in voices, the whole
  // voice description for VoiceDesign, and for cloning the thing that usually
  // gets read out loud instead of followed.
  const instruct = (MODE==='design') ? $('design').value : $('instruct').value
  return {text:$('text').value, mode:$('mode').value, instruct:instruct,
    speaker:(MODE==='custom' ? $('speaker').value : ''),
    xvec:(MODE==='clone' && $('xvec').checked),
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
// Two different facts, and conflating them disabled the load button on a
// model that was merely SELECTED: /health reports `model` - which checkpoint
// the sidecar is set to - and `ok` - whether its weights are actually in
// video memory. The header said "модель не в памяти" while the button beside
// it said "загружена" and refused to be pressed.
let MODELS=[], CURRENT_MODEL='', MODEL_IN_MEMORY=false
const DOT='\u00b7'
async function loadModels(){
  try{
    const d=await (await fetch('/models')).json()
    MODELS=d.known
    const sel=$('model_id')
    sel.innerHTML=MODELS.map(m=>
      '<option value="'+m.id+'"'+(m.id===d.current?' selected':'')+'>'
      +m.id.replace('Qwen/Qwen3-TTS-12Hz-','')+(m.clones?'':'  \u2014 без клонирования')
      +(m.on_disk ? '  \u00b7 на диске' : '  \u00b7 надо скачать')
      +'</option>').join('')
    showModelNote()
  }catch(e){}
}
function showModelNote(){
  // the note for the model the MODE points at, not for whatever the hidden
  // picker happens to hold - they parted ways when the picker stopped being
  // the thing anybody touched
  const m=MODELS.find(x=>x.id===(typeof modeModelId==='function' ? modeModelId() : $('model_id').value))
  $('model_note').textContent=m?m.note:''
  $('model_note').className='meta'+(m&&!m.clones?' warn':'')
}
async function applyModel(asked){
  const id=$('model_id').value
  const m=MODELS.find(x=>x.id===id)
  // Asked only when the model is chosen without saying why. Coming from a mode
  // strip, the mode already said it: picking "Голос по описанию" IS the
  // statement that the reference is not being used, and a dialog repeating it
  // on every press is a dialog people dismiss without reading - which is how a
  // press ended up doing nothing at all.
  if(!asked && m && !m.clones &&
     !confirm('Эта модель не клонирует голос по образцу. Джарвис будет говорить '
              +'чужим голосом. Всё равно загрузить?')) return
  const note=$('model_note')
  note.className='meta'
  note.textContent = (m && !m.on_disk)
    ? 'скачиваю модель, это несколько гигабайт \u2014 ход внизу под кнопками загрузки'
    : 'загружаю \u2014 ход внизу под кнопками загрузки'
  try{
    const r=await fetch('/model',{method:'POST',headers:{'Content-Type':'application/json'},
      body:JSON.stringify({model_id:id})})
    const d=await r.json()
    if(d.error){ note.className='meta warn'; note.textContent=d.error; return }
    // The request comes straight back now - the load runs on a thread. Reading
    // "changed" here is what made this report "уже была загружена" for a model
    // that had not been fetched at all. Follow /health instead, closely, until
    // the loader admits it has started.
    watchLoad(true)
    for(let i=0;i<10;i++){
      await new Promise(r2=>setTimeout(r2,400))
      await refresh()
      if(BUSY) break
    }
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
let gpuTimer=null
// The rows as the server sent them. Kept so a click can name the process
// without putting that name into an onclick attribute: a name containing
// a quote would break the markup, and escaping one is not possible here -
// this whole page is a Python string, and Python consumes the escape first.
let GPU=[]
function showTab(which){
  for(const name of ['tts','pack','gpu']){
    $('pane-'+name).style.display = (name===which) ? '' : 'none'
    $('tab-'+name).className = (name===which) ? 'tab on' : 'tab'
  }
  // Reading the counters costs a second of a Windows utility's time, so it
  // happens while that tab is open and not otherwise.
  clearInterval(gpuTimer); gpuTimer=null
  if(which==='gpu'){ gpuLoad(); gpuTimer=setInterval(gpuLoad, 5000) }
  if(which==='pack'){ packLoad() }
}
function esc(s){ return String(s).replace(/[&<>"]/g, c =>
  ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c])) }
// Biggest first, because the question being asked is always "who is eating
// the card". Clicking a heading again turns that column around.
// ---------------------------------------------------------------- the pack
let PACK=[], PACKSORT='', PACKDIR=1, BAKING=false, STOPBAKE=false, CANBAKE=true

function packListDraw(packs, current){
  const sel = $('packpick')
  sel.innerHTML = packs.map(p =>
    '<option value="'+esc(p.folder)+'"'+(p.folder===current?' selected':'')+'>'
    + esc(p.name) + ' \u00b7 ' + p.clips + ' записей</option>').join('')
  $('packlist').innerHTML = packs.map(p =>
    '<div>' + (p.current ? '<b>' : '') + esc(p.name) + (p.current ? '</b>' : '')
    + ' \u2014 <code>' + esc(p.folder) + '</code>, '
    + p.clips + ' записей, ' + p.lines + ' реплик, языки: ' + esc(p.languages.join(', '))
    + '</div>').join('')
}

async function packSelect(){
  try{
    await fetch('/voices/select',{method:'POST',headers:{'Content-Type':'application/json'},
      body:JSON.stringify({id:$('packpick').value})})
  }catch(e){}
  packLoad()
}

async function newVoice(){
  const id = prompt('Имя папки для нового голоса (латиница, цифры, дефис):', 'jarvis-new')
  if(!id) return
  const name = prompt('Как показывать его в настройках Джарвиса:', id) || id
  try{
    const r = await fetch('/voices/new',{method:'POST',headers:{'Content-Type':'application/json'},
      body:JSON.stringify({id:id.trim(), name:name.trim()})})
    const j = await r.json()
    if(!r.ok){ packStatus(j.error || ('ошибка '+r.status), true); return }
    packStatus('голос ' + j.created + ' заведён по образцу ' + j.from
      + ' \u2014 записей пока нет, начитайте их кнопками ниже')
  }catch(e){ packStatus('сайдкар не отвечает', true); return }
  packLoad()
}

async function dropVoice(){
  const id = $('packpick').value
  if(!confirm('Удалить голос ' + id + ' вместе со всеми его записями? Это навсегда.')) return
  try{
    const r = await fetch('/voices/delete',{method:'POST',headers:{'Content-Type':'application/json'},
      body:JSON.stringify({id:id})})
    const j = await r.json()
    if(!r.ok){ packStatus(j.error || ('ошибка '+r.status), true); return }
    packStatus('голос ' + j.deleted + ' удалён')
  }catch(e){ packStatus('сайдкар не отвечает', true); return }
  packLoad()
}

async function packLoad(){
  try{
    const p = await (await fetch('/pack')).json()
    if(p.packs) packListDraw(p.packs, p.id)
    PACK = p.rows
    CANBAKE = p.can_bake !== false
    $('packmeta').textContent = p.name + ' · эталон ' + p.reference
      + ' ' + p.slice.secs + 's · ' + p.rows.length + ' клипов'
      + (p.loaded ? '' : ' · модель не в памяти, первая запись её загрузит')
    if(!CANBAKE){
      packStatus('Загружена ' + (p.model||'').split('/').pop()
        + ' — она не клонирует голос по образцу, начитать заготовки ею нельзя. '
        + 'Выберите модель -Base на вкладке «Синтез».', true)
    }
    packDraw()
  }catch(e){ $('packmeta').textContent='сайдкар не отвечает' }
}

function packSort(key){
  if(PACKSORT===key){ PACKDIR = -PACKDIR } else { PACKSORT = key; PACKDIR = 1 }
  packDraw()
}

function packDraw(){
  const k=PACKSORT, d=PACKDIR
  if(k){
    PACK.sort((a,b) => {
      if(k==='secs') return ((a.secs||0)-(b.secs||0))*d
      const x=String(a[k]).toLowerCase(), y=String(b[k]).toLowerCase()
      return x<y ? -d : (x>y ? d : 0)
    })
  }
  for(const c of ['reaction','stem','secs']){
    $('pr-'+c).textContent = (c===k) ? (d>0 ? '▲' : '▼') : ''
  }
  $('packrows').innerHTML = PACK.map((r, i) => {
    const secs = r.secs ? r.secs.toFixed(2)+'с' : '—'
    const tag = r.orphan ? '<span class="why">ничья</span>' : esc(r.reaction)
    return '<tr id="pk'+i+'"><td>'+tag+'</td><td>'+esc(r.stem)+'</td>'
      + '<td><input id="pt'+i+'" value="'+esc(r.text)+'" onchange="packSave('+i+')"></td>'
      + '<td class="num">'+secs+'</td>'
      + '<td class="act">'
      + (r.exists ? '<button class="small ghost" onclick="packPlay('+i+')">▶</button> ' : '')
      + '<button class="small ghost" onclick="packBake('+i+')"'
      + (CANBAKE ? '' : ' disabled title="нужна клонирующая модель"')
      + '>Записать</button>'
      + '</td></tr>'
  }).join('')
}

function packPlay(i){
  const r = PACK[i]
  const a = $('packaudio') || Object.assign(document.createElement('audio'), {id:'packaudio'})
  if(!a.parentNode) document.body.appendChild(a)
  // the file changes under the same name every time it is baked, so the
  // browser has to be told this is not the one it already has
  a.src = '/pack/audio?stem=' + encodeURIComponent(r.stem) + '&v=' + Date.now()
  a.play()
}

async function packSave(i){
  const r = PACK[i], text = $('pt'+i).value
  if(text === r.text) return
  try{
    await fetch('/pack/line',{method:'POST',headers:{'Content-Type':'application/json'},
      body:JSON.stringify({stem:r.stem, text:text})})
    r.text = text
    packStatus('текст ' + r.stem + ' сохранён')
  }catch(e){ packStatus('не сохранить текст', true) }
}

function packStatus(msg, bad){
  const st=$('packstatus'); st.className = bad ? 'meta warn' : 'meta'; st.textContent = msg
}

async function bakeOne(i){
  const r = PACK[i], text = ($('pt'+i)||{}).value || r.text
  const row = $('pk'+i); if(row) row.style.opacity = '.5'
  try{
    const res = await fetch('/pack/bake',{method:'POST',
      headers:{'Content-Type':'application/json'},
      body:JSON.stringify({stem:r.stem, text:text})})
    const j = await res.json()
    if(!res.ok){ packStatus(r.stem + ': ' + (j.error || res.status), true); return false }
    r.text = j.text; r.secs = j.secs; r.exists = true
    packStatus(r.stem + ' записан за ' + j.took_secs + 'с (' + j.secs + 'с звука)')
    return true
  }catch(e){ packStatus(r.stem + ': сайдкар не ответил', true); return false }
  finally{ if(row) row.style.opacity = '' }
}

async function packBake(i){
  if(BAKING) return
  BAKING = true
  packStatus('записываю ' + PACK[i].stem + '... первая запись грузит модель, это долго')
  await bakeOne(i)
  BAKING = false
  packDraw()
}

async function bakeAll(){
  if(BAKING) return
  if(!CANBAKE){
    packStatus('Сначала загрузите клонирующую модель -Base на вкладке «Синтез».', true)
    return
  }
  if(!confirm('Перезаписать все ' + PACK.length + ' заготовок? Это займёт минуты. '
    + 'Прежние файлы сохранятся рядом с пакетом.')) return
  BAKING = true; STOPBAKE = false
  $('bakeall').textContent = 'Остановить'
  $('bakeall').onclick = () => { STOPBAKE = true }
  let done = 0
  for(let i=0; i<PACK.length; i++){
    if(STOPBAKE){ packStatus('остановлено после ' + done + ' из ' + PACK.length); break }
    packStatus('записываю ' + (i+1) + ' из ' + PACK.length + ': ' + PACK[i].stem)
    if(await bakeOne(i)) done++
  }
  if(!STOPBAKE) packStatus('готово: ' + done + ' из ' + PACK.length)
  $('bakeall').textContent = 'Перегенерировать всё'
  $('bakeall').onclick = bakeAll
  BAKING = false
  packDraw()
}

// ----------------------------------------------------------- video memory
let SORTKEY='mb', SORTDIR=-1
let CANKILL=true

function sortBy(key){
  if(SORTKEY===key){ SORTDIR = -SORTDIR }
  else { SORTKEY = key; SORTDIR = (key==='name') ? 1 : -1 }
  drawGpu()
}

function drawGpu(){
  const k=SORTKEY, d=SORTDIR
  GPU.sort((a,b) => {
    if(k==='name'){
      const x=String(a.name).toLowerCase(), y=String(b.name).toLowerCase()
      return x<y ? -d : (x>y ? d : 0)
    }
    return (a[k]-b[k])*d
  })

  for(const c of ['name','pid','mb']){
    $('ar-'+c).textContent = (c===k) ? (d>0 ? '▲' : '▼') : ''
  }

  // The index below is into GPU as it stands AFTER this sort, and gpuKill
  // reads the same array - so re-sorting can never point a button at the
  // wrong process.
  $('gpurows').innerHTML = GPU.map((p, i) => {
    const cls = p.blocked==='self' ? 'mine' : (p.blocked ? 'blocked' : '')
    let act = '<button class="small danger" onclick="gpuKill(' + i + ')">Завершить</button>'
    if(p.blocked==='self') act = '<span class="why">это сайдкар</span>'
    else if(p.blocked)     act = '<span class="why">системный</span>'
    else if(!CANKILL)      act = '<span class="why">не на петле</span>'
    const full = esc(p.path || p.name)
    return '<tr class="'+cls+'"><td title="'+full+'"><div class="pname">'+esc(p.name)+'</div>'
      + (p.path ? '<div class="why">'+full+'</div>' : '')
      + '</td><td class="num">'+p.pid+'</td>'
    + '<td class="num">'+(p.mb ? p.mb+' МБ' : 'меньше МБ')+'</td>'
      + '<td class="act">'+act+'</td></tr>'
  }).join('')
}

async function gpuLoad(){
  try{
    const g = await (await fetch('/gpu')).json()
    const v = g.vram||{}
    CANKILL = g.can_kill
    $('gpucard').textContent = v.card_total_mb
      ? ('карта занята на '+v.card_used_mb+' МБ из '+v.card_total_mb+', свободно '+v.card_free_mb)
      : 'карта не видна'
    if(!g.supported){
      $('gpurows').innerHTML='<tr><td colspan="4" class="meta">Счётчики видеопамяти '
        +'по процессам есть только в Windows.</td></tr>'
      return
    }
    if(!g.processes.length){
      $('gpurows').innerHTML='<tr><td colspan="4" class="meta">Карту никто не держит.</td></tr>'
      return
    }
    GPU = g.processes
    drawGpu()
  }catch(e){ $('gpucard').textContent='сайдкар не отвечает' }
}
async function gpuKill(i){
  const p = GPU[i]
  if(!p) return
  const pid = p.pid, name = p.name
  if(!confirm('Завершить ' + name + ' (pid ' + pid + ')? Несохранённое в нём будет потеряно.')) return
  const st=$('gpustatus'); st.className='meta'; st.textContent='завершаю '+name+'...'
  try{
    const r = await fetch('/gpu/kill',{method:'POST',headers:{'Content-Type':'application/json'},
      body:JSON.stringify({pid:pid})})
    const j = await r.json()
    if(!r.ok){ st.className='meta warn'; st.textContent = j.error || ('ошибка '+r.status) }
    else{ st.className='meta'; st.textContent = j.ended+' завершён, держал '+j.freed_mb+' МБ' }
  }catch(e){ st.className='meta warn'; st.textContent='не дозвониться до сайдкара' }
  finally{ gpuLoad() }
}
setMode('clone')
refresh(); setInterval(refresh, 5000)
</script></body></html>
"""


@app.get("/", response_class=HTMLResponse)
def index():
    return PAGE


def main():
    global CFG, _slice_path, _slice_secs, _prompt_text, BOUND_HOST
    ap = argparse.ArgumentParser()
    # Anything other than the loopback turns OFF process ending - see the note
    # on BOUND_HOST. The rest of the console keeps working.
    ap.add_argument("--host", default="127.0.0.1")
    # a different port from the CosyVoice sidecar on purpose: both can run, and
    # switching engines is then one setting rather than a restart dance
    ap.add_argument("--port", type=int, default=8772)
    # The 1.7B is what ships. It is about twice the size of the 0.6B and
    # noticeably steadier: better prosody, and far less variation from take
    # to take. A saved config beats this, so an existing install keeps
    # whatever was chosen in the console.
    ap.add_argument("--model-id", default="Qwen/Qwen3-TTS-12Hz-1.7B-Base")
    # Start without the model in video memory.
    #
    # Default, and not a small thing: the model is about 5 GB, and taking it at
    # startup means the sidecar competes for the card with whatever is already
    # using it. Started while a game held the card, the load did not finish in
    # three minutes and the launcher gave up on a process that was alive and
    # merely thrashing. Nothing else needs it that early - the console answers,
    # the video memory tab works, and the model arrives when it is asked for.
    ap.add_argument("--preload", action="store_true",
                    help="load the model at startup instead of on the first request")
    ap.add_argument("--language", default="Russian", choices=SUPPORTED_LANGUAGES)
    # Which built-in voice, for the CustomVoice models. Ignored by the cloning
    # ones, which take their voice from the reference recording instead.
    ap.add_argument("--speaker", default="")
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
    if SAVED_LANGUAGE:
        a.language = SAVED_LANGUAGE
    if SAVED_SPEAKER:
        a.speaker = SAVED_SPEAKER

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

    out = HERE / "out" / "_sidecar_qwen_ref.wav"
    out.parent.mkdir(exist_ok=True)
    _slice_path, _slice_secs = refslice.slice_reference(
        a.reference, a.start, a.length, out, do_snap=a.snap)
    print(f"reference : {a.reference.name}  {a.start}+{a.length}s "
          f"-> {_slice_secs:.2f}s  snap={a.snap}")
    _prompt_text = a.prompt_text.strip() or transcribe(_slice_path)
    print(f"transcript: {_prompt_text}")

    if a.preload:
        model()              # warm before serving

        # ...and warm the path that will actually be used. The model's own
        # warmup() does not touch voice cloning, so the first cloned request
        # still cost 5.7 s against the 0.43 s every one after it - and the
        # first request is precisely the one somebody is watching. One
        # throwaway line here moves that cost off the first question.
        try:
            t0 = time.time()
            for _ in synth_stream("Готово."):
                pass
            print(f"cloning path warmed in {time.time()-t0:.1f}s", flush=True)
        except Exception as e:
            print(f"warm-up generation failed, first answer will be slow: {e}",
                  flush=True)
    else:
        print("model not loaded - press «Загрузить в память» in the console, "
              "or just ask for speech and it will load itself", flush=True)

    print(f"http://{a.host}:{a.port}", flush=True)

    # what the kill endpoint checks before it will do anything
    BOUND_HOST = a.host
    if a.host not in LOOPBACK:
        print("[gpu] listening on {} - ending processes from the console is "
              "off".format(a.host), flush=True)

    import uvicorn
    uvicorn.run(app, host=a.host, port=a.port, log_level="warning")
    return 0


if __name__ == "__main__":
    sys.exit(main())
