"""
The parts of a speech sidecar that have nothing to do with which model it runs.

Two sidecars exist because their Python dependencies cannot share a room:
CosyVoice pins transformers 4.51.3, qwen-tts wants 4.57.3. Splitting the
environments is cheap; letting the two copies of the wire format and the
quality checks drift apart is not. So everything a sidecar does BESIDES
synthesis lives here, and both import it.
"""
import hashlib
import io
import struct
from pathlib import Path

import numpy as np
import soundfile as sf

# ----------------------------------------------------------------- frames
# The response body is a sequence of length-prefixed frames, so one reader on
# the Rust side serves every engine and every mode: one-shot sends a single
# frame, streaming sends many. A zero length ends the stream, which is also
# how the client tells "finished" from "died mid-answer".
FLAG_FINAL = 1 << 0
FLAG_FELL_BACK = 1 << 1       # asked for streaming, got nothing, used one shot

END = struct.pack("<II", 0, FLAG_FINAL)


def frame(payload: bytes, flags: int = 0) -> bytes:
    return struct.pack("<II", len(payload), flags) + payload


def wav_bytes(audio: np.ndarray, sr: int) -> bytes:
    buf = io.BytesIO()
    sf.write(buf, audio, sr, format="WAV", subtype="PCM_16")
    return buf.getvalue()


# ------------------------------------------------------------ take checks
VOWELS = set("аеёиоуыэюяaeiouy")


def syllables(text: str) -> int:
    """rough syllable count - one per vowel, close enough in Russian"""
    return sum(1 for c in text.lower() if c in VOWELS)


# Speaking rate bounds, in syllables per second. Ordinary Russian runs 4-6; a
# deliberate butler's delivery sits near 3.5. Both ends matter and only one was
# watched at first: the pack's worst clip crawled at 0.87 syl/s, four times
# slower than speech, because nothing rejected a drawl. 3.4 sits just under the
# clips judged fine by ear.
MIN_SYL_PER_SEC = 3.4
MAX_SYL_PER_SEC = 8.0


def speech_span(audio: np.ndarray, sr: int, drop_db: float = 35.0):
    """seconds from the first audible frame to the last, ignoring the silence"""
    w = max(1, int(sr * 0.01))
    n = (audio.size // w) * w
    if n == 0:
        return 0.0
    db = 20 * np.log10(np.sqrt((audio[:n].reshape(-1, w) ** 2).mean(axis=1)) + 1e-12)
    loud = np.where(db > db.max() - drop_db)[0]
    if loud.size == 0:
        return 0.0
    return (loud[-1] - loud[0] + 1) * w / sr


def bad_take(audio: np.ndarray, sr: int, text: str):
    """
    Why this take cannot be used, or None if it is fine.

    Cheap checks only - a whisper pass would cost more than the synthesis it
    guards. These catch what actually happens: silence, a stub far too short
    for the words asked for, and a drawl far too long.

    Measured against the SPEAKING span rather than the file length. Judging by
    file length lets a take that is mostly leading silence pass on duration
    while carrying almost no speech, which is how a 0.12 s "Да, сэр." once got
    through a rate check written to catch exactly that.
    """
    if audio.size == 0:
        return "empty"
    peak = float(np.abs(audio).max())
    if peak < 0.01:
        return f"silent (peak {peak:.4f})"

    secs = speech_span(audio, sr)
    if secs <= 0.0:
        return "no speech in the take"
    syl = syllables(text)
    if syl == 0:
        return None if secs >= 0.2 else f"stub ({secs:.2f}s)"

    rate = syl / secs
    if rate > MAX_SYL_PER_SEC:
        return f"gabbled ({rate:.1f} syl/s over {secs:.2f}s)"
    if rate < MIN_SYL_PER_SEC:
        return f"dragging ({rate:.1f} syl/s over {secs:.2f}s, want >= {MIN_SYL_PER_SEC})"
    return None


def trim_head(audio: np.ndarray, sr: int, lead=0.12, drop_db=45.0):
    """
    Drop the dead air in front of the first frame of an utterance.

    Measured on CosyVoice: frame one carries 0.5-1.0 s of silence before the
    first word and every later frame carries none at either edge, so the
    stitching is naturally seamless and only the very start is wasted.

    Deliberately conservative. The threshold is low and 120 ms of room tone is
    kept, because a Russian line can open on a quiet fricative (с, ф, х) far
    below the peak; cutting flush would eat it.
    """
    if audio.size == 0:
        return audio
    w = max(1, int(sr * 0.01))
    n = (audio.size // w) * w
    if n == 0:
        return audio
    db = 20 * np.log10(np.sqrt((audio[:n].reshape(-1, w) ** 2).mean(axis=1)) + 1e-12)
    loud = np.where(db > db.max() - drop_db)[0]
    if not loud.size:
        return audio
    return audio[max(0, int(loud[0] * w - lead * sr)):]


# ------------------------------------------------------------- transcripts
def transcript_cache(slice_path, root=None):
    """
    Where the transcript of THIS AUDIO lives, keyed by what the audio contains.

    Keyed by content and not by filename on purpose. Both sidecars cut the
    same slice with the same code and get byte-identical audio, but each
    writes it under its own name - so a filename key made the second one look
    the transcript up, miss, and need whisper for a value that already
    existed. Whisper is a heavy thing to install twice to recompute a sentence.
    """
    data = Path(slice_path).read_bytes()
    digest = hashlib.sha256(data).hexdigest()[:16]
    root = Path(root) if root else Path(slice_path).parent.parent
    return root / f"reference_text_{digest}.txt"
