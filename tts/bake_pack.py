"""
Bake a complete voice pack with the cloned voice.

The measurement said short phrases cost ~2 s to synthesise, which is useless at
runtime - but every canned reaction is known in advance, so they can be built
once, offline, and played as ordinary files by the code that already exists.
No sidecar, no GPU at runtime, no Rust changes.

    python bake_pack.py <reference.wav> [out_dir]
"""
import os
import sys
import time
from pathlib import Path

HERE = Path(__file__).parent
# Model caches live with every other model, one folder up. Set before any
# huggingface import: these are read once, at import time, and a late change
# has no effect at all.
CACHE = HERE.parent / "models" / "sidecar"
COSY = HERE / "CosyVoice"

os.environ.setdefault("HF_HOME", str(CACHE / "hf"))
os.environ.setdefault("MODELSCOPE_CACHE", str(CACHE / "modelscope"))

# torchaudio 2.11 routes audio IO through TorchCodec, which needs system FFmpeg.
# soundfile is already a dependency and carries libsndfile with it.
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

if len(sys.argv) < 2:
    raise SystemExit("usage: python bake_pack.py <reference.wav> [out_dir]")
REFERENCE = Path(sys.argv[1])
OUT = Path(sys.argv[2]) if len(sys.argv) > 2 else HERE / "pack" / "ru"
OUT.mkdir(parents=True, exist_ok=True)

# reaction -> [(file stem, line), ...]
# Written in Jarvis's register: unhurried, formal, never chatty. Several
# variants per reaction so the assistant does not repeat itself.
PHRASES = {
    "greet":        [("run", "К вашим услугам, сэр.")],
    "greet_morning": [("greet_morning", "Доброе утро, сэр. Все системы в норме.")],
    "greet_day":    [("greet_day", "Добрый день, сэр.")],
    "greet_evening": [("greet_evening", "Добрый вечер, сэр.")],
    "greet_night":  [("greet_night", "Доброй ночи, сэр. Я не сплю.")],
    "reply": [
        ("reply1", "Слушаю, сэр."),
        ("reply2", "Да, сэр."),
        ("reply3", "К вашим услугам."),
    ],
    "ok": [
        ("ok1", "Выполнено, сэр."),
        ("ok2", "Готово."),
        ("ok3", "Запрос выполнен, сэр."),
        ("ok4", "Сделано."),
    ],
    "thinking": [
        ("thinking1", "Думаю над ответом, сэр."),
        ("thinking2", "Секунду, сэр."),
        ("thinking3", "Обрабатываю запрос."),
    ],
    "not_found": [
        ("not_found1", "Я не понял команду, сэр."),
        ("not_found2", "Такой команды у меня нет."),
    ],
    "thanks": [
        ("thanks1", "Всегда рад помочь, сэр."),
        ("thanks2", "Не за что."),
    ],
    "error": [
        ("error1", "Есть небольшая проблема, сэр."),
        ("error2", "Команду выполнить не удалось."),
    ],
    "goodbye": [
        ("goodbye1", "До свидания, сэр."),
        ("goodbye2", "Отключаюсь."),
    ],
}

print(f"reference : {REFERENCE}")
print(f"output    : {OUT}")

# ------------------------------------------------------------------ transcript
transcript_file = HERE / f"reference_text_{REFERENCE.stem}.txt"
if transcript_file.exists():
    prompt_text = transcript_file.read_text(encoding="utf-8").strip()
else:
    import whisper
    print("transcribing the reference...")
    w = whisper.load_model("small")
    prompt_text = w.transcribe(str(REFERENCE), language="ru")["text"].strip()
    del w
    transcript_file.write_text(prompt_text, encoding="utf-8")

# ---------------------------------------------------------------------- model
os.chdir(COSY)
from cosyvoice.cli.cosyvoice import AutoModel

t = time.time()
cosyvoice = AutoModel(model_dir=str(MODEL_DIR), fp16=True)
print(f"model loaded in {time.time()-t:.1f}s\n")

# CosyVoice3 asserts on this marker; prompt_text is an instruction plus the
# reference transcript, not the transcript alone
tagged = f"You are a helpful assistant.<|endofprompt|>{prompt_text}"

total = 0.0
made = []
for reaction, items in PHRASES.items():
    for stem, line in items:
        t = time.time()
        chunks = [o["tts_speech"] for o in
                  cosyvoice.inference_zero_shot(line, tagged, str(REFERENCE), stream=False)]
        audio = _torch.cat(chunks, dim=1)
        path = OUT / f"{stem}.wav"
        _ta.save(str(path), audio, cosyvoice.sample_rate)
        dur = audio.shape[1] / cosyvoice.sample_rate
        took = time.time() - t
        total += took
        made.append((reaction, stem))
        print(f"  {reaction:<14} {stem:<14} {dur:4.2f}s  ({took:4.1f}s)  {line}")

print(f"\n{len(made)} clips in {total:.0f}s -> {OUT}")

# ------------------------------------------------------------------ voice.toml
lines = [
    '[voice]',
    'id = "jarvis-og-tts"',
    'name = "Jarvis OG (synth)"',
    'author = ""',
    'languages = ["ru"]',
    '',
    '[reactions.ru]',
]
for reaction, items in PHRASES.items():
    names = ", ".join(f'"{s}"' for s, _ in items)
    lines.append(f'{reaction} = [{names}]')

toml_path = OUT.parent / "voice.toml"
toml_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
print(f"wrote {toml_path}")
