"""
Re-bake the voice pack with Qwen3-TTS.

Same lines, same reference slice, same checks - only the engine changes.
The lines come from the pack itself now; see _pack_lines below.
One-shot mode on purpose: it is the path where a take can be inspected and
retaken, and for baking, latency does not matter at all while a bad clip lives
in the pack forever.

Written over the existing pack id rather than into a new one so no setting has
to change; the previous pack is copied aside first so the swap is reversible.

One line here is not the line that was wanted. "Обрабатываю запрос" came out
as "Апробатываю" on twelve attempts across four temperatures - a stable
refusal on one word, not bad luck - so it is "Работаю над запросом" instead.
The words are ours to choose; the pack only names the file.
"""
import io
import json
import shutil
import struct
import sys
import time
import urllib.request
import difflib
import re
from pathlib import Path

import numpy as np
import soundfile as sf

API = "http://127.0.0.1:8772"
PACK = Path(r"I:\jarvis\resources\sound\voices\jarvis-og-tts")
# beside this script, wherever it happens to live. It used to name the
# directory the scripts were developed in, which quietly became the wrong
# machine's path the moment they moved into the project.
BACKUP = Path(__file__).parent / "pack-previous"
ATTEMPTS = 6

# Temperature matters more than attempts for word accuracy: at 0.9 the model
# wanders ("Слушаюсь" for "Слушаю"), lower samples less adventurously. Walked
# rather than fixed, because too cold has its own failures.
TEMPS = [0.9, 0.6, 0.75, 0.45]

# What to say, read from the pack rather than kept here.
#
# It used to be a dict in this file, which meant the pack on disk could not say
# what any of its own clips contained and the console had nothing to show. The
# text lives in voice.toml now, beside the recordings of it, and both this
# script and the console read that one table - so a line edited in the console
# is the line this script bakes.
def _pack_lines():
    import tomlkit
    doc = tomlkit.parse((PACK / "voice.toml").read_text(encoding="utf-8"))
    table = doc.get("lines", {})
    lines = (table.get("ru", {}) if table else {}) or {}
    if not lines:
        sys.exit(f"{PACK / 'voice.toml'} has no [lines.ru] table - nothing to say")
    return {str(k): str(v) for k, v in lines.items()}


LINES = _pack_lines()

VOWELS = set("аеёиоуыэюя")
syl = lambda s: sum(1 for c in s.lower() if c in VOWELS)
norm = lambda s: re.sub(r"[^а-я ]+", " ", s.lower().replace("ё", "е")).split()


def rd(r, n):
    b = b""
    while len(b) < n:
        p = r.read(n - len(b))
        if not p:
            return None
        b += p
    return b


def speak(text, temperature=0.9):
    req = urllib.request.Request(
        API + "/speak",
        data=json.dumps({"text": text, "mode": "sentence",
                         "temperature": temperature}).encode(),
        headers={"Content-Type": "application/json"})
    parts, ended = [], False
    with urllib.request.urlopen(req, timeout=600) as r:
        while True:
            h = rd(r, 8)
            if h is None:
                break
            ln, _f = struct.unpack("<II", h)
            if ln == 0:
                ended = True
                break
            p = rd(r, ln)
            if p is None:
                break
            parts.append(p)
    return (parts[0] if parts and ended else None)


def measure(wav, text):
    x, sr = sf.read(io.BytesIO(wav), dtype="float32", always_2d=True)
    m = x.mean(axis=1)
    w = max(1, int(sr * 0.01))
    n = (m.size // w) * w
    db = 20 * np.log10(np.sqrt((m[:n].reshape(-1, w) ** 2).mean(axis=1)) + 1e-12)
    loud = np.where(db > db.max() - 35)[0]
    secs = (loud[-1] - loud[0] + 1) * w / sr if loud.size else m.size / sr
    return syl(text) / secs, secs


try:
    h = json.load(urllib.request.urlopen(API + "/health", timeout=10))
    print(f'engine {h.get("engine")} · {h.get("model")} · reference {h["slice"]["secs"]}s')
except Exception as e:
    sys.exit(f"qwen sidecar not answering: {e}")

# keep the old pack so the swap can be undone
if not BACKUP.exists():
    shutil.copytree(PACK, BACKUP)
    print(f"previous pack copied to {BACKUP}")
else:
    print(f"previous pack already backed up at {BACKUP}")

import whisper
w = whisper.load_model("small")

print(f'\n{"stem":<15}{"try":>4}{"syl/s":>7}{"secs":>7}{"match":>7}  line')
print("-" * 76)
t0 = time.time()
results = []
for stem, line in LINES.items():
    best = None
    for attempt in range(1, ATTEMPTS + 1):
        wav = speak(line, TEMPS[min(attempt - 1, len(TEMPS) - 1)])
        if wav is None:
            print(f'{stem:<15}{attempt:>4}   rejected by the sidecar, asking again')
            continue
        tmp = PACK / "_take.wav"
        tmp.write_bytes(wav)
        heard = w.transcribe(str(tmp), language="ru")["text"].strip()
        m = difflib.SequenceMatcher(None, norm(line), norm(heard)).ratio()
        rate, secs = measure(wav, line)
        if best is None or m > best[0]:
            best = (m, rate, secs, wav, heard)
        print(f'{stem:<15}{attempt:>4}{rate:7.2f}{secs:7.2f}{m:7.2f}  '
              f'{line if m >= 0.95 else repr(heard)}')
        if m >= 0.95:
            break
    tmp = PACK / "_take.wav"
    if tmp.exists():
        tmp.unlink()
    if best is None:
        print(f"{stem:<15}  GAVE UP - old clip left in place")
        results.append((stem, None, None))
        continue
    (PACK / "ru" / f"{stem}.wav").write_bytes(best[3])
    results.append((stem, best[0], best[1]))

ok = [r for r in results if r[1] is not None]
good = [r for r in ok if r[1] >= 0.95]
print(f"\n{len(ok)}/{len(LINES)} clips written in {time.time()-t0:.0f}s")
if ok:
    rates = [r[2] for r in ok]
    print(f"spoken correctly: {len(good)}/{len(ok)}")
    print(f"speaking rate: median {np.median(rates):.2f}, "
          f"slowest {min(rates):.2f}, fastest {max(rates):.2f} syl/s")
weak = [r[0] for r in ok if r[1] < 0.95]
if weak:
    print(f"worth listening to: {weak}")
failed = [r[0] for r in results if r[1] is None]
if failed:
    print(f"NOT regenerated: {failed}")
