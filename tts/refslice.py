"""
Cutting a usable reference out of a long recording.

Shared by the studio and the sidecar so the two cannot drift: the voice the
assistant speaks with at runtime has to be the voice the baked clips were made
from, and that means the same slice, produced by the same code.
"""
import numpy as np
import soundfile as sf


def energy_db(x, sr, win_ms=10):
    """level of each short window, in dB"""
    win = max(1, int(sr * win_ms / 1000))
    n = (x.size // win) * win
    if n == 0:
        return np.zeros(0), win
    rms = np.sqrt((x[:n].reshape(-1, win) ** 2).mean(axis=1) + 1e-12)
    return 20 * np.log10(rms + 1e-12), win


def snap(x, sr, idx, at_start, search_s=3.0, min_pause_ms=120, drop_db=9.0):
    """
    Move idx to the nearest real pause, searching up to search_s away.

    This is what stops the model prepending a phantom word to every
    generation. It is handed the reference audio AND its transcript and
    continues from them, so a slice that ends mid-word makes it finish that
    word first - you hear a stray "сесть" before your line. Cleaning the
    output cannot help: that syllable is correctly synthesised, it is just not
    yours.

    Two things this deliberately does not do. It does not test against a fixed
    dB floor: film dialogue carries room tone, and the sample this was written
    for sits at -26 dB median, so any absolute threshold is either deaf or
    trigger-happy. A pause is defined relative to the speech around it. And it
    does not accept a single quiet frame - that is as likely to be the closure
    of a plosive as a word boundary, so the quiet has to hold for
    min_pause_ms.
    """
    db, win = energy_db(x, sr)
    need = max(1, int(min_pause_ms * sr / (win * 1000)))
    if db.size < need + 2:
        return idx

    here = min(max(idx // win, 0), db.size - 1)
    span = max(need + 1, int(search_s * sr / win))
    lo = max(0, here - span)
    hi = min(db.size, here + span)
    if hi - lo < need:
        return idx

    # loudest frame inside each candidate pause, judged against the level of
    # ordinary speech nearby
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


def slice_reference(path, start, length, out_path, do_snap=True):
    """cut [start, start+length) out of the reference and write it to out_path"""
    data, sr = sf.read(str(path), dtype="float32", always_2d=True)
    mono = data.mean(axis=1)
    a = int(max(0.0, start) * sr)
    b = len(mono) if length <= 0 else min(len(mono), a + int(length * sr))

    if do_snap:
        # the end matters most - that is the edge the model continues from
        a = snap(mono, sr, a, at_start=True)
        if b < len(mono):
            b = snap(mono, sr, b, at_start=False)
        if b <= a:
            b = min(len(mono), a + int(max(length, 6.0) * sr))

    cut = mono[a:b]
    peak = np.abs(cut).max() if cut.size else 0.0
    if peak > 0:
        cut = cut * (0.95 / peak)
    # a beat of silence at the tail so the boundary reads as a full stop
    cut = np.concatenate([cut, np.zeros(int(sr * 0.25), dtype="float32")])
    sf.write(str(out_path), cut, sr)
    return out_path, len(cut) / sr
