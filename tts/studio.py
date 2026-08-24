"""
Local voice studio for CosyVoice.

Loads the model once and keeps it warm, so each generation costs the ~2 s of
inference rather than the ~10 s of startup. Serves one page on
http://127.0.0.1:8770 where you pick a reference, choose which slice of it to
use, type a line and listen.

    python studio.py
"""
import io
import os
import sys
import time
import threading
from pathlib import Path

HERE = Path(__file__).parent
COSY = HERE / "CosyVoice"
MODEL_DIR = HERE / "models" / "Fun-CosyVoice3-0.5B"
SAMPLES = [HERE / "xamples", HERE / "examples", HERE]
OUT_DIR = HERE / "studio_out"
OUT_DIR.mkdir(exist_ok=True)

os.environ.setdefault("HF_HOME", str(HERE / "hf"))
os.environ.setdefault("MODELSCOPE_CACHE", str(HERE / "modelscope"))

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

from fastapi import FastAPI, Body
from fastapi.responses import HTMLResponse, Response, JSONResponse

app = FastAPI()
_model = None
_lock = threading.Lock()          # one GPU, one generation at a time
_whisper = None


def model():
    global _model
    if _model is None:
        os.chdir(COSY)
        from cosyvoice.cli.cosyvoice import AutoModel
        t = time.time()
        _model = AutoModel(model_dir=str(MODEL_DIR), fp16=True)
        print(f"model loaded in {time.time()-t:.1f}s", flush=True)
    return _model


def find_references():
    seen, out = set(), []
    for d in SAMPLES:
        if not d.is_dir():
            continue
        for p in sorted(d.glob("*.wav")) + sorted(d.glob("*.mp3")) + sorted(d.glob("*.flac")):
            if p.name.startswith(("t0_", "studio_")) or p.resolve() in seen:
                continue
            seen.add(p.resolve())
            try:
                info = sf.info(str(p))
                out.append({"path": str(p), "name": p.name, "secs": round(info.duration, 2)})
            except Exception:
                pass
    return out


def _energy_db(x, sr, win_ms=10):
    win = max(1, int(sr * win_ms / 1000))
    n = (x.size // win) * win
    if n == 0:
        return np.zeros(0), win
    rms = np.sqrt((x[:n].reshape(-1, win) ** 2).mean(axis=1) + 1e-12)
    return 20 * np.log10(rms + 1e-12), win


def _snap(x, sr, idx, at_start, search_s=3.0, min_pause_ms=120, drop_db=9.0):
    """
    Move idx to the nearest real pause, searching up to search_s away.

    This is the fix for the phantom word at the start of every generation. The
    model is handed the reference audio AND its transcript and continues from
    them, so a slice that ends mid-word makes it finish that word first - you
    hear a stray "сесть" before your line. Cleaning the output cannot help:
    that syllable is correctly synthesised, it is just not yours.

    Two things this does not do. It does not test against a fixed dB floor:
    film dialogue carries room tone, and this sample sits at -26 dB median, so
    any absolute threshold is either deaf or trigger-happy. A pause is defined
    relative to the speech around it. And it does not accept a single quiet
    frame - that is as likely to be the closure of a plosive as a word
    boundary. The quiet has to hold for min_pause_ms.
    """
    db, win = _energy_db(x, sr)
    need = max(1, int(min_pause_ms * sr / (win * 1000)))
    if db.size < need + 2:
        return idx

    here = min(max(idx // win, 0), db.size - 1)
    span = max(need + 1, int(search_s * sr / win))
    lo = max(0, here - span)
    hi = min(db.size, here + span)
    if hi - lo < need:
        return idx

    # loudest frame inside each candidate pause, and the level of ordinary
    # speech nearby to judge it against
    runs = np.lib.stride_tricks.sliding_window_view(db[lo:hi], need).max(axis=1)
    speech = np.median(db[lo:hi])
    ok = np.where(runs <= speech - drop_db)[0]
    if ok.size == 0:
        return idx

    starts = ok + lo
    j = int(starts[np.argmin(np.abs(starts - here))])   # never travel further than needed
    quiet = db <= speech - drop_db
    keep = int(0.040 * sr)                              # a beat of room tone reads as natural

    if at_start:
        k = j + need
        while k < db.size and quiet[k]:                 # the pause may run longer than need
            k += 1
        return max(0, k * win - keep)                   # begin as the next word begins
    return min(x.size, j * win + keep)                  # end just inside the pause


def slice_reference(path, start, length, snap=True):
    """cut [start, start+length) out of the reference and hand back a temp wav"""
    data, sr = sf.read(str(path), dtype="float32", always_2d=True)
    mono = data.mean(axis=1)
    a = int(max(0.0, start) * sr)
    b = len(mono) if length <= 0 else min(len(mono), a + int(length * sr))

    if snap:
        # the end matters most - that is the edge the model continues from
        a = _snap(mono, sr, a, at_start=True)
        if b < len(mono):
            b = _snap(mono, sr, b, at_start=False)
        if b <= a:
            b = min(len(mono), a + int(max(length, 6.0) * sr))

    cut = mono[a:b]
    peak = np.abs(cut).max() if cut.size else 0.0
    if peak > 0:
        cut = cut * (0.95 / peak)
    # leave a beat of silence at the tail so the boundary reads as a full stop
    cut = np.concatenate([cut, np.zeros(int(sr * 0.25), dtype="float32")])
    tmp = OUT_DIR / "_ref_slice.wav"
    sf.write(tmp, cut, sr)
    return tmp, len(cut) / sr


def transcribe(path):
    global _whisper
    cache = OUT_DIR / f"tr_{Path(path).stem}_{int(os.path.getmtime(path))}.txt"
    if cache.exists():
        return cache.read_text(encoding="utf-8").strip()
    import whisper
    if _whisper is None:
        _whisper = whisper.load_model("small")
    text = _whisper.transcribe(str(path), language="ru")["text"].strip()
    cache.write_text(text, encoding="utf-8")
    return text


def clean(audio, sr, head_db=-38.0, tail_pad_ms=180):
    """
    Two fixes for what zero-shot models do at the edges: a click or a breath
    before the first word, and a final consonant clipped off. Trim the head
    down to where speech actually starts, and never trim the tail - pad it,
    so a swallowed ending has somewhere to live.
    """
    x = audio.astype("float32")
    win = max(1, int(sr * 0.010))
    n = (x.size // win) * win
    if n:
        rms = np.sqrt((x[:n].reshape(-1, win) ** 2).mean(axis=1) + 1e-12)
        db = 20 * np.log10(rms + 1e-12)
        loud = np.where(db > head_db)[0]
        if loud.size:
            start = max(0, loud[0] * win - int(sr * 0.020))
            x = x[start:]
    # a short fade-in kills any residual click without eating the first phoneme
    fade = min(int(sr * 0.008), x.size)
    if fade:
        x[:fade] *= np.linspace(0.0, 1.0, fade, dtype="float32")
    return np.concatenate([x, np.zeros(int(sr * tail_pad_ms / 1000), dtype="float32")])


@app.get("/api/references")
def api_references():
    return {"references": find_references()}


@app.post("/api/generate")
def api_generate(body: dict = Body(...)):
    ref = body.get("reference", "")
    text = (body.get("text") or "").strip()
    start = float(body.get("start", 0))
    length = float(body.get("length", 0))
    prompt_text = (body.get("prompt_text") or "").strip()
    do_clean = bool(body.get("clean", True))
    do_snap = bool(body.get("snap", True))
    tail_ms = int(body.get("tail_ms", 180))

    if not ref or not Path(ref).exists():
        return JSONResponse({"error": "reference not found"}, status_code=400)
    if not text:
        return JSONResponse({"error": "text is empty"}, status_code=400)

    with _lock:
        t0 = time.time()
        slice_path, slice_secs = slice_reference(ref, start, length, snap=do_snap)
        if not prompt_text:
            prompt_text = transcribe(slice_path)
        # an unterminated transcript invites the model to continue it
        if prompt_text and prompt_text[-1] not in ".!?…":
            prompt_text += "."

        # CosyVoice3 asserts on this marker
        tagged = f"You are a helpful assistant.<|endofprompt|>{prompt_text}"

        m = model()
        # token2wav trips over a degenerate final chunk every dozen or so
        # generations ("kernel size can't be greater than actual input size").
        # It is sampling-dependent, so the same request usually succeeds on a
        # second pass - cheaper to retry than to hand back a 500.
        last = None
        for _ in range(3):
            try:
                chunks = [o["tts_speech"] for o in
                          m.inference_zero_shot(text, tagged, str(slice_path), stream=False)]
                break
            except RuntimeError as e:
                last = e
        else:
            return JSONResponse({"error": f"vocoder failed 3 times: {last}"}, status_code=500)
        audio = torch.cat(chunks, dim=1).squeeze(0).cpu().numpy()
        sr = m.sample_rate
        if do_clean:
            audio = clean(audio, sr, tail_pad_ms=tail_ms)
        took = time.time() - t0

    buf = io.BytesIO()
    sf.write(buf, audio, sr, format="WAV", subtype="PCM_16")
    import base64
    return {
        "wav": base64.b64encode(buf.getvalue()).decode(),
        "secs": round(len(audio) / sr, 2),
        "took": round(took, 2),
        "prompt_text": prompt_text,
        "slice_secs": round(slice_secs, 2),
    }


@app.post("/api/save")
def api_save(body: dict = Body(...)):
    import base64
    name = (body.get("name") or "clip").strip().replace("/", "_").replace("\\", "_")
    data = base64.b64decode(body.get("wav", ""))
    path = OUT_DIR / f"{name}.wav"
    path.write_bytes(data)
    return {"saved": str(path)}


@app.post("/api/transcribe")
def api_transcribe(body: dict = Body(...)):
    """
    Show the transcript before generating, not after.

    The transcript is the other half of the phantom-word bug: whatever it says
    that the audio does not actually contain, the model speaks first. Being
    able to read it - and correct it by hand - is the difference between
    guessing and knowing.
    """
    ref = body.get("reference", "")
    if not ref or not Path(ref).exists():
        return JSONResponse({"error": "reference not found"}, status_code=400)
    with _lock:
        slice_path, slice_secs = slice_reference(
            ref, float(body.get("start", 0)), float(body.get("length", 0)),
            snap=bool(body.get("snap", True)))
        text = transcribe(slice_path)
        if text and text[-1] not in ".!?…":
            text += "."
    return {"prompt_text": text, "slice_secs": round(slice_secs, 2)}


@app.get("/api/reference_audio")
def api_reference_audio(path: str, start: float = 0, length: float = 0, snap: bool = True):
    p, _ = slice_reference(path, start, length, snap=snap)
    return Response(p.read_bytes(), media_type="audio/wav")


PAGE = """
<!doctype html><html lang="ru"><head><meta charset="utf-8">
<title>Voice studio</title>
<style>
:root{--bg:#0e1416;--card:#151e21;--line:#243034;--ink:#dde7ea;--soft:#8fa5aa;--acc:#45d6e2}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--ink);font:15px/1.55 system-ui,sans-serif;padding:28px}
h1{font-size:19px;margin:0 0 4px;letter-spacing:-.01em}
.sub{color:var(--soft);font-size:13px;margin-bottom:22px}
.wrap{max-width:900px;margin:0 auto}
.card{background:var(--card);border:1px solid var(--line);border-radius:8px;padding:18px;margin-bottom:16px}
label{display:block;font-size:12px;text-transform:uppercase;letter-spacing:.08em;color:var(--soft);margin-bottom:6px}
select,input,textarea{width:100%;background:#0e1416;color:var(--ink);border:1px solid var(--line);
  border-radius:5px;padding:9px 11px;font:inherit}
textarea{resize:vertical;min-height:74px}
.row{display:flex;gap:12px;flex-wrap:wrap}
.row>div{flex:1;min-width:130px}
button{background:var(--acc);color:#06231f;border:0;border-radius:5px;padding:10px 18px;
  font:600 14px system-ui;cursor:pointer}
button.ghost{background:transparent;color:var(--acc);border:1px solid var(--line)}
button:disabled{opacity:.5;cursor:default}
.bar{display:flex;gap:10px;align-items:center;margin-top:14px;flex-wrap:wrap}
.meta{color:var(--soft);font-size:12.5px;font-variant-numeric:tabular-nums}
.item{border-top:1px solid var(--line);padding:12px 0;display:flex;gap:12px;align-items:center;flex-wrap:wrap}
.item .txt{flex:1;min-width:200px}
audio{height:34px}
.err{color:#ec8175}
code{background:#0e1416;padding:1px 5px;border-radius:3px;font-size:12.5px}
</style></head><body><div class="wrap">
<h1>Voice studio</h1>
<div class="sub">CosyVoice 3, модель держится загруженной. Одна генерация за раз.</div>

<div class="card">
  <label>Эталон</label>
  <select id="ref"></select>
  <div class="row" style="margin-top:12px">
    <div><label>Начало, с</label><input id="start" type="number" value="0" step="0.5" min="0"></div>
    <div><label>Длина, с (0 = до конца)</label><input id="length" type="number" value="8" step="1" min="0"></div>
    <div><label>Хвост, мс</label><input id="tail" type="number" value="180" step="20" min="0"></div>
  </div>
  <div class="bar">
    <button class="ghost" onclick="previewRef()">Послушать отрезок</button>
    <button class="ghost" id="tr" onclick="doTranscribe()">Расшифровать</button>
    <span id="refmeta" class="meta"></span>
  </div>
  <div class="meta" style="margin-top:8px">8–12 с — устойчиво. На 15–20 с вокодер
    срывается в огрызок или падает.</div>
  <div style="margin-top:10px"><audio id="refaudio" controls style="width:100%"></audio></div>
  <div style="margin-top:12px">
    <label>Расшифровка эталона</label>
    <textarea id="prompt" style="min-height:56px"
      placeholder="пусто = распознает whisper"></textarea>
    <div class="meta" style="margin-top:6px">Должна совпадать с отрезком слово в слово.
      Лишнее слово в конце модель договорит в начале твоей реплики.</div>
  </div>
</div>

<div class="card">
  <label>Текст</label>
  <textarea id="text">Думаю над ответом, сэр.</textarea>
  <div class="bar">
    <button id="go" onclick="gen()">Сгенерировать</button>
    <label style="display:inline;text-transform:none;letter-spacing:0;color:var(--ink)">
      <input type="checkbox" id="clean" style="width:auto"> чистить края
    </label>
    <label style="display:inline;text-transform:none;letter-spacing:0;color:var(--ink)">
      <input type="checkbox" id="snap" checked style="width:auto"> резать эталон по паузам
    </label>
    <span id="status" class="meta"></span>
  </div>
</div>

<div class="card"><div id="list"></div></div>
</div>
<script>
let refs=[]
async function loadRefs(){
  const r=await (await fetch('/api/references')).json()
  refs=r.references
  document.getElementById('ref').innerHTML=refs.map(x=>
    `<option value="${x.path}">${x.name} — ${x.secs}s</option>`).join('')
  showMeta()
}
function showMeta(){
  const p=document.getElementById('ref').value
  const f=refs.find(x=>x.path===p)
  document.getElementById('refmeta').textContent=f?`полная длина ${f.secs}s`:''
}
document.getElementById('ref').addEventListener('change',()=>{showMeta();dropStale()})
// a transcript belongs to one slice; changing the slice invalidates it
for(const id of ['start','length','snap'])
  document.getElementById(id).addEventListener('change',dropStale)
function dropStale(){
  const p=document.getElementById('prompt')
  if(p.dataset.auto==='1'){p.value='';p.dataset.auto=''}
}
async function doTranscribe(){
  const b=document.getElementById('tr'), st=document.getElementById('refmeta')
  b.disabled=true; const was=st.textContent; st.textContent='распознаю...'
  try{
    const r=await (await fetch('/api/transcribe',{method:'POST',
      headers:{'Content-Type':'application/json'},body:JSON.stringify(slice())})).json()
    if(r.error){st.textContent=r.error; return}
    const p=document.getElementById('prompt')
    p.value=r.prompt_text; p.dataset.auto='1'
    st.textContent=was+` · отрезок ${r.slice_secs}s`
  }finally{b.disabled=false}
}
function slice(){return{reference:document.getElementById('ref').value,
  start:+document.getElementById('start').value,
  length:+document.getElementById('length').value,
  snap:document.getElementById('snap').checked}}
function previewRef(){
  const q=new URLSearchParams({path:document.getElementById('ref').value,
    start:document.getElementById('start').value,length:document.getElementById('length').value,
    snap:document.getElementById('snap').checked})
  document.getElementById('refaudio').src='/api/reference_audio?'+q
}
async function gen(){
  const btn=document.getElementById('go'), st=document.getElementById('status')
  btn.disabled=true; st.textContent='генерация...'; st.className='meta'
  try{
    const body={reference:document.getElementById('ref').value,
      text:document.getElementById('text').value,
      start:+document.getElementById('start').value,
      length:+document.getElementById('length').value,
      tail_ms:+document.getElementById('tail').value,
      prompt_text:document.getElementById('prompt').value,
      clean:document.getElementById('clean').checked,
      snap:document.getElementById('snap').checked}
    const r=await fetch('/api/generate',{method:'POST',headers:{'Content-Type':'application/json'},
      body:JSON.stringify(body)})
    const d=await r.json()
    if(d.error){st.textContent=d.error; st.className='meta err'; return}
    st.textContent=`${d.took}s на ${d.secs}s звука, эталон ${d.slice_secs}s`
    const pf=document.getElementById('prompt')
    if(!pf.value){pf.value=d.prompt_text; pf.dataset.auto='1'}
    addItem(body.text,d)
  }catch(e){st.textContent=String(e); st.className='meta err'}
  finally{btn.disabled=false}
}
function addItem(text,d){
  const el=document.createElement('div'); el.className='item'
  const name=text.replace(/[^\\wа-яА-ЯёЁ ]/g,'').trim().split(/\\s+/).slice(0,3).join('_')||'clip'
  el.innerHTML=`<div class="txt">${text}<div class="meta">${d.secs}s · ${d.took}s</div></div>
    <audio controls src="data:audio/wav;base64,${d.wav}"></audio>
    <input style="width:150px" value="${name}">
    <button class="ghost">Сохранить</button>`
  el.querySelector('button').onclick=async(e)=>{
    const n=el.querySelector('input').value
    const r=await (await fetch('/api/save',{method:'POST',headers:{'Content-Type':'application/json'},
      body:JSON.stringify({name:n,wav:d.wav})})).json()
    e.target.textContent=r.saved?'сохранено':'ошибка'
  }
  document.getElementById('list').prepend(el)
}
loadRefs()
</script></body></html>
"""


@app.get("/", response_class=HTMLResponse)
def index():
    return PAGE


if __name__ == "__main__":
    import uvicorn
    print("loading model before serving, so the first request is not the slow one...")
    model()
    os.chdir(HERE)
    print("http://127.0.0.1:8770", flush=True)
    uvicorn.run(app, host="127.0.0.1", port=8770, log_level="warning")
