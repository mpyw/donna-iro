#!/usr/bin/env python3
"""MusicXML から確認用のリファレンス音源を書き出す。

**つくよみちゃんの声ではない。** 単純な合成音でメロディと伴奏を鳴らすだけ。
編曲した色が音数的に破綻していないか、拍に乗っているかを
OpenUtau で調声する前に耳で確かめるためのもの。

出力は assets/reference/*.wav（gitignore 済み。再生成できるので）。

    python3 tools/render_reference.py
"""

import glob
import os
import wave
import xml.etree.ElementTree as ET

import numpy as np

SRC = "assets/musicxml"
OUT = "assets/reference"
SR = 44100
BPM = 126
DIV = 12
SEC_PER_DIV = (60.0 / BPM) / DIV

STEP = {"C": 0, "D": 2, "E": 4, "F": 5, "G": 7, "A": 9, "B": 11}


def freq(pitch):
    midi = (12 * (int(pitch.findtext("octave")) + 1)
            + STEP[pitch.findtext("step")]
            + int(pitch.findtext("alter") or 0))
    return 440.0 * 2 ** ((midi - 69) / 12.0)


def tone(f, dur, gain, harmonics, decay):
    """減衰する倍音つきの音を1つ作る。"""
    n = max(1, int(dur * SR))
    t = np.arange(n) / SR
    wave_ = np.zeros(n)
    for k, amp in enumerate(harmonics, start=1):
        wave_ += amp * np.sin(2 * np.pi * f * k * t)
    env = np.exp(-decay * t)
    # 頭のプチッというノイズを抑える
    atk = min(n, int(0.005 * SR))
    env[:atk] *= np.linspace(0, 1, atk)
    rel = min(n, int(0.02 * SR))
    env[-rel:] *= np.linspace(1, 0, rel)
    return wave_ * env * gain


def collect(part):
    """(開始divisions, 長さdivisions, pitch要素) を集める。

    backup / forward を追ってピアノの2声部も正しい位置に置く。
    """
    events = []
    base = 0
    for m in part.findall("measure"):
        pos = 0
        prev = 0
        for el in m:
            if el.tag == "note":
                d = int(el.findtext("duration") or 0)
                if el.find("chord") is not None:
                    start = prev            # 和音は直前の音と同時
                else:
                    start = pos
                    prev = pos
                    pos += d
                p = el.find("pitch")
                if p is not None:
                    events.append((base + start, d, p))
            elif el.tag == "backup":
                pos -= int(el.findtext("duration"))
            elif el.tag == "forward":
                pos += int(el.findtext("duration"))
        base += 48
    return events, base


def render(path):
    root = ET.parse(path).getroot()
    voices = {
        # メロディ: 明るめ、長めに伸びる
        "P1": (dict(gain=0.55, harmonics=[1.0, 0.35, 0.15], decay=1.6), None),
        # 伴奏: 控えめ、速く減衰
        "P2": (dict(gain=0.16, harmonics=[1.0, 0.2], decay=4.5), None),
    }
    total_div = 0
    plans = []
    for pid, (params, _) in voices.items():
        part = root.find(f"part[@id='{pid}']")
        if part is None:
            continue
        events, span = collect(part)
        total_div = max(total_div, span)
        plans.append((events, params))

    # 小節長ちょうどで切る。余韻を足すと素材を繋いだときに
    # そこが無音の隙間になり、曲として繋がらなくなる。
    # 節の末尾はもともと休符で終わっているので、切っても不自然にならない。
    n = int(round(total_div * SEC_PER_DIV * SR))
    buf = np.zeros(n)
    for events, params in plans:
        for start, dur, p in events:
            s = int(start * SEC_PER_DIV * SR)
            # 音価より少し長く鳴らして自然な減衰にする（末尾ははみ出た分を切る）
            length = dur * SEC_PER_DIV * 1.3
            sig = tone(freq(p), length, **params)
            end = min(n, s + len(sig))
            buf[s:end] += sig[: end - s]

    peak = np.abs(buf).max()
    if peak > 0:
        buf = buf / peak * 0.89

    # 先頭の無音を落とす。原曲 m1 は両パートとも3拍が空で、
    # そのまま出すと再生開始時に間の抜けた沈黙になる。
    nz = np.nonzero(np.abs(buf) > 1e-4)[0]
    if len(nz):
        buf = buf[nz[0]:]
        n = len(buf)

    # 末尾で減衰が残っていた場合のプチッというノイズだけ潰す。
    # 15ms なら繋ぎ目として知覚されない。
    fade = min(n, int(0.015 * SR))
    buf[-fade:] *= np.linspace(1, 0, fade)
    pcm = (buf * 32767).astype("<i2")

    os.makedirs(OUT, exist_ok=True)
    name = os.path.basename(path).replace(".musicxml", ".wav")
    dst = os.path.join(OUT, name)
    with wave.open(dst, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm.tobytes())
    return dst, len(pcm) / SR


def main():
    for path in sorted(glob.glob(os.path.join(SRC, "*.musicxml"))):
        dst, sec = render(path)
        print(f"{os.path.basename(dst):<22} {sec:5.2f}s")


if __name__ == "__main__":
    main()
