"""
Measure what the voice pipeline actually waits on.

Time-to-first-token is the usual benchmark and it is the wrong one here: you
cannot synthesise half a sentence. What matters is how long until the first
COMPLETE sentence exists, because that is when CosyVoice can start, and how
fast the rest arrives after that, because that decides whether synthesis or
generation is the bottleneck.

The token is read from the environment and never printed. Set it in the shell
you run this from - it does not need to go anywhere else:

    $env:LM_API_TOKEN = "<your token>"
    I:\jarvis-tts\venv\Scripts\python.exe I:\jarvis-tts\bench_llm.py

    # optional: a different model or endpoint
    python bench_llm.py --model gemma-4-e4b-it-heretic --url http://127.0.0.1:1234/v1
"""
import argparse
import json
import os
import re
import sys
import time
import urllib.error
import urllib.request

SENT_END = re.compile(r"[.!?…](\s|$)|[:;]\s")

PROMPTS = [
    ("короткий", "Сколько оперативной памяти считается достаточным для игр в 2026 году?"),
    ("средний", "Объясни в двух-трёх предложениях, чем SSD NVMe отличается от SATA."),
    ("длинный", "Расскажи, как проверить, что видеокарта работает исправно."),
]

SYSTEM = ("Ты — голосовой ассистент Джарвис. Отвечай по-русски, кратко и по делу, "
          "обычной речью без списков и разметки.")


def first_sentence_end(text):
    m = SENT_END.search(text)
    return m.end() if m else None


def run(url, model, token, prompt, stream, max_tokens):
    body = {
        "model": model,
        "messages": [{"role": "system", "content": SYSTEM},
                     {"role": "user", "content": prompt}],
        "max_tokens": max_tokens,
        "temperature": 0.7,
        "stream": stream,
    }
    req = urllib.request.Request(
        url.rstrip("/") + "/chat/completions",
        data=json.dumps(body).encode("utf-8"),
        headers={"Content-Type": "application/json",
                 "Authorization": f"Bearer {token}"})

    t0 = time.perf_counter()
    text = ""
    t_first_token = None
    t_first_sentence = None
    first_sentence = ""
    n_chunks = 0

    with urllib.request.urlopen(req, timeout=300) as r:
        if not stream:
            d = json.load(r)
            text = (d["choices"][0]["message"].get("content") or "").strip()
            total = time.perf_counter() - t0
            usage = d.get("usage") or {}
            cut = first_sentence_end(text)
            return {"ttft": None, "t_sentence": total, "total": total,
                    "text": text, "first_sentence": text[:cut] if cut else text,
                    "completion_tokens": usage.get("completion_tokens"),
                    "chunks": None}
        for raw in r:
            line = raw.decode("utf-8", "replace").strip()
            if not line.startswith("data:"):
                continue
            payload = line[5:].strip()
            if payload == "[DONE]":
                break
            try:
                d = json.loads(payload)
            except json.JSONDecodeError:
                continue
            delta = (d["choices"][0].get("delta") or {}).get("content") or ""
            if not delta:
                continue
            n_chunks += 1
            if t_first_token is None:
                t_first_token = time.perf_counter() - t0
            text += delta
            if t_first_sentence is None:
                cut = first_sentence_end(text)
                if cut:
                    t_first_sentence = time.perf_counter() - t0
                    first_sentence = text[:cut].strip()

    total = time.perf_counter() - t0
    return {"ttft": t_first_token, "t_sentence": t_first_sentence, "total": total,
            "text": text.strip(), "first_sentence": first_sentence or text.strip(),
            "completion_tokens": None, "chunks": n_chunks}


def list_models(url, token):
    req = urllib.request.Request(url.rstrip("/") + "/models",
                                 headers={"Authorization": f"Bearer {token}"})
    with urllib.request.urlopen(req, timeout=30) as r:
        return [m["id"] for m in json.load(r).get("data", [])]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--url", default="http://127.0.0.1:1234/v1")
    ap.add_argument("--model", default="gemma-4-e4b-it-heretic")
    ap.add_argument("--max-tokens", type=int, default=2048)
    a = ap.parse_args()

    token = os.environ.get("LM_API_TOKEN", "")
    if not token:
        print('LM_API_TOKEN is not set. In PowerShell:\n'
              '    $env:LM_API_TOKEN = "<your token>"', file=sys.stderr)
        return 2

    try:
        available = list_models(a.url, token)
    except Exception as e:
        print(f"cannot reach {a.url}: {type(e).__name__}: {e}", file=sys.stderr)
        return 1
    if not available:
        print("the endpoint reports no models loaded", file=sys.stderr)
        return 1

    # exact id, else a single unambiguous loose match. Never fall back to
    # "whatever is loaded": this endpoint also serves an ASR model and an
    # embedding model, and silently benchmarking one of those produces
    # numbers that look real and mean nothing.
    model = a.model
    if model not in available:
        loose = [m for m in available if a.model.lower() in m.lower()]
        if len(loose) == 1:
            print(f"'{a.model}' not found exactly, using '{loose[0]}'")
            model = loose[0]
        else:
            print(f"model '{a.model}' is not loaded. Available:", file=sys.stderr)
            for m in available:
                print(f"    {m}", file=sys.stderr)
            print("\nPass one with --model <id>.", file=sys.stderr)
            return 1

    print(f"endpoint : {a.url}")
    print(f"model    : {model}")
    print(f"loaded   : {', '.join(available)}\n")
    a.model = model

    rows = []
    for label, prompt in PROMPTS:
        for stream in (True, False):
            try:
                r = run(a.url, a.model, token, prompt, stream, a.max_tokens)
            except urllib.error.HTTPError as e:
                detail = e.read().decode("utf-8", "replace")[:200]
                print(f"{label:<9} stream={str(stream):<5} HTTP {e.code}: {detail}")
                continue
            except Exception as e:
                print(f"{label:<9} stream={str(stream):<5} {type(e).__name__}: {e}")
                continue
            rows.append((label, stream, r))
            ttft = f"{r['ttft']*1000:6.0f}" if r["ttft"] is not None else "     -"
            tsent = f"{r['t_sentence']*1000:6.0f}" if r["t_sentence"] else "     -"
            print(f"{label:<9} stream={str(stream):<5} "
                  f"first token {ttft} ms   first sentence {tsent} ms   "
                  f"total {r['total']*1000:6.0f} ms   {len(r['text']):4d} chars")
            print(f'          first sentence: {r["first_sentence"][:80]!r}')

    if not rows:
        return 1

    print("\n" + "=" * 74)
    print("what this means for the voice pipeline")
    print("=" * 74)
    st = [r for _, s, r in rows if s and r["t_sentence"]]
    ns = [r for _, s, r in rows if not s]
    if st:
        best = min(r["t_sentence"] for r in st)
        worst = max(r["t_sentence"] for r in st)
        print(f"streaming: first sentence ready in {best*1000:.0f}-{worst*1000:.0f} ms")
    if ns:
        best = min(r["total"] for r in ns)
        worst = max(r["total"] for r in ns)
        print(f"one shot : whole answer ready in  {best*1000:.0f}-{worst*1000:.0f} ms")
    if st and ns:
        gain = (min(r["total"] for r in ns) - min(r["t_sentence"] for r in st)) * 1000
        print(f"\nstreaming buys roughly {gain:.0f} ms before synthesis can start.")
        print("CosyVoice needs ~2000 ms for its first sentence either way, and the")
        print("'thinking' clip covers 1.6-2.8 s of that. If the gain above is small,")
        print("SSE parsing is not worth the code - take the answer whole and chunk it.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
