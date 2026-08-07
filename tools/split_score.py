#!/usr/bin/env python3
"""元譜面から素材ごとの MusicXML を切り出す。

原曲にある4色（あか・あお・きいろ・みどり）はそのまま抜き出し、
残り8色は節の型に音数を合わせて編曲する。

出力は assets/musicxml/*.musicxml。OpenUtau に読み込ませて
つくよみちゃんUTAU音源で歌わせる想定。

    python3 tools/split_score.py
"""

import copy
import os
import xml.etree.ElementTree as ET

SRC = "assets/score/donna-irogasuki.musicxml"
OUT = "assets/musicxml"
BPM = 126
DIV = 12  # divisions per quarter note

# ---------------------------------------------------------------- 音価

# duration(divisions) -> (note type, dots)
DUR_TYPE = {
    48: ("whole", 0), 36: ("half", 1), 24: ("half", 0), 18: ("quarter", 1),
    12: ("quarter", 0), 9: ("eighth", 1), 6: ("eighth", 0),
    4: ("16th", 0), 3: ("16th", 0), 2: ("32nd", 0), 1: ("32nd", 0),
}


def set_duration(note, d):
    """note の duration/type/dot を d に揃える。"""
    note.find("duration").text = str(d)
    typ, dots = DUR_TYPE.get(d, ("eighth", 0))
    t = note.find("type")
    if t is None:
        t = ET.SubElement(note, "type")
    t.text = typ
    for old in note.findall("dot"):
        note.remove(old)
    for _ in range(dots):
        note.append(ET.Element("dot"))


def split_dur(d):
    """d を2つに割る。付点のスウィング感を保つよう 3:1 を優先する。"""
    if d % 4 == 0 and d // 4 >= 3:
        return d * 3 // 4, d // 4
    if d % 3 == 0 and d // 3 >= 3:
        return d * 2 // 3, d // 3
    return d - d // 2, d // 2


# ---------------------------------------------------------------- 読み込み

def load():
    tree = ET.parse(SRC)
    root = tree.getroot()
    parts = {p.get("id"): p for p in root.findall("part")}
    measures = {
        pid: {m.get("number"): m for m in p.findall("measure")}
        for pid, p in parts.items()
    }
    return root, measures


def pitched(measure):
    """和音構成音を除いた、旋律として並ぶ有音高の音符。"""
    return [n for n in measure.findall("note")
            if n.find("pitch") is not None and n.find("chord") is None]


# ---------------------------------------------------------------- 組み立て

def attributes_el(fifths):
    a = ET.Element("attributes")
    ET.SubElement(a, "divisions").text = str(DIV)
    k = ET.SubElement(a, "key")
    ET.SubElement(k, "fifths").text = str(fifths)
    t = ET.SubElement(a, "time")
    ET.SubElement(t, "beats").text = "4"
    ET.SubElement(t, "beat-type").text = "4"
    return a


def tempo_el():
    d = ET.Element("direction", {"placement": "above"})
    dt = ET.SubElement(d, "direction-type")
    mm = ET.SubElement(dt, "metronome")
    ET.SubElement(mm, "beat-unit").text = "quarter"
    ET.SubElement(mm, "per-minute").text = str(BPM)
    ET.SubElement(d, "sound", {"tempo": str(BPM)})
    return d


def mute_melody(measure, beat_from=0.0):
    """指定拍以降の有音高の音符を休符に置き換える（歌詞も落とす）。"""
    pos = 0.0
    for n in measure.findall("note"):
        dur = int(n.findtext("duration") or 0)
        if n.find("chord") is None:
            start = pos
            pos += dur / DIV
        else:
            start = pos - dur / DIV
        if n.find("pitch") is not None and start >= beat_from - 1e-6:
            n.remove(n.find("pitch"))
            n.insert(0, ET.Element("rest"))
            for ly in n.findall("lyric"):
                n.remove(ly)


def build(root, measures, name, spans, fifths, mods=None):
    """spans = [(part追従する小節番号のリスト)] を1本のスコアに組む。"""
    out = ET.Element("score-partwise", {"version": "4.0"})
    w = ET.SubElement(out, "work")
    ET.SubElement(w, "work-title").text = f"どんな色がすき - {name}"
    out.append(copy.deepcopy(root.find("part-list")))

    for pid in ("P1", "P2"):
        part = ET.SubElement(out, "part", {"id": pid})
        for i, num in enumerate(spans):
            src = measures[pid].get(num)
            m = copy.deepcopy(src) if src is not None else ET.Element("measure")
            m.set("number", str(i + 1))
            # 元の attributes / direction は落として作り直す
            for tag in ("attributes", "direction", "print", "barline"):
                for el in m.findall(tag):
                    m.remove(el)
            if i == 0:
                m.insert(0, attributes_el(fifths))
                m.insert(1, tempo_el())
            if mods:
                mods(pid, num, m)
            part.append(m)
    return out


def write(out, name):
    os.makedirs(OUT, exist_ok=True)
    ET.indent(out, space="  ")
    path = os.path.join(OUT, f"{name}.musicxml")
    ET.ElementTree(out).write(path, encoding="UTF-8", xml_declaration=True)
    return path


# ---------------------------------------------------------------- 歌詞の当て直し

def set_lyric(note, text):
    for ly in note.findall("lyric"):
        note.remove(ly)
    ly = ET.SubElement(note, "lyric", {"number": "1"})
    ET.SubElement(ly, "syllabic").text = "single"
    ET.SubElement(ly, "text").text = text


def refit(measure, syllables):
    """小節内の旋律音を音数に合わせて分割・結合し、歌詞を当て直す。

    増やすときは一番長い音符を 3:1 に割る（原曲が「きいろ」で
    D6 を割っているのと同じ操作）。減らすときは短い隣接音を束ねる。
    """
    notes = pitched(measure)
    target = len(syllables)

    while len(notes) < target:
        i = max(range(len(notes)),
                key=lambda k: int(notes[k].findtext("duration")))
        src = notes[i]
        d = int(src.findtext("duration"))
        a, b = split_dur(d)
        dup = copy.deepcopy(src)
        set_duration(src, a)
        set_duration(dup, b)
        idx = list(measure).index(src)
        measure.insert(idx + 1, dup)
        notes = pitched(measure)

    while len(notes) > target:
        i = min(range(len(notes) - 1),
                key=lambda k: int(notes[k].findtext("duration"))
                + int(notes[k + 1].findtext("duration")))
        keep, drop = notes[i], notes[i + 1]
        set_duration(keep, int(keep.findtext("duration"))
                     + int(drop.findtext("duration")))
        measure.remove(drop)
        notes = pitched(measure)

    for n, s in zip(notes, syllables):
        set_lyric(n, s)


def refit_verse(mm, l1, l3):
    """6小節の節（mm[0..5]）に歌詞を当て直す。

    1行目は mm[0] に l1[:-1]、最後の1音は mm[1] の頭。
    3行目は mm[4] に l3[:-1]、最後の1音は mm[5] の頭。
    2行目（いちばんさきになくなるよ）は全節共通なので触らない。
    """
    refit(mm[0], l1[:-1])
    set_lyric(pitched(mm[1])[0], l1[-1])
    refit(mm[4], l3[:-1])
    set_lyric(pitched(mm[5])[0], l3[-1])


def mute_before(measure, beat):
    """指定拍より前の有音高の音符を休符にする。"""
    pos = 0.0
    for n in measure.findall("note"):
        dur = int(n.findtext("duration") or 0)
        start = pos
        if n.find("chord") is None:
            pos += dur / DIV
        else:
            start = pos - dur / DIV
        if n.find("pitch") is not None and start < beat - 1e-6:
            n.remove(n.find("pitch"))
            n.insert(0, ET.Element("rest"))
            for ly in n.findall("lyric"):
                n.remove(ly)


def rng(a, b):
    return [str(i) for i in range(a, b + 1)]


# 原曲にある4色。歌詞はそのままで音数も合っているので触らない。
ORIGINAL = {
    "red": rng(6, 11),
    "blue": rng(14, 19),
    "yellow": rng(22, 27),
    "green": rng(34, 39),
}

# 原曲にない8色。(1行目, 3行目) の音節。
# 「ちゃ」「みー」は1音に乗せるので1要素として扱う。
NEW = {
    "yellowgreen": (list("きみどりいろがすき"), list("きみどりのクレヨン")),
    "white": (list("しーろいいろがすき"), list("しろいクレヨン")),
    "black": (list("くろいいろがすき"), list("くろいクレヨン")),
    "pink": (list("ピンクいろがすき"), list("ピンクのクレヨン")),
    "orange": (list("オレンジいろがすき"), list("オレンジのクレヨン")),
    "purple": (list("むらさきいろがすき"), list("むらさきのクレヨン")),
    "brown": (["ちゃ"] + list("いろいいろがすき"), ["ちゃ"] + list("いろいクレヨン")),
    "lightblue": (list("みーずいろがすき"), list("みずいろのクレヨン")),
}

# 1行目の音価を明示的に上書きする色（divisions 単位、合計 48）。
#
# 「しーろ」「みーず」は頭の母音を伸ばす。UTAU では「ー」を
# 独立した音符に置くと直前の母音を継続するので、そう表現する。
# 「し」と「ー」で 9+9 = 18（1.5拍）を使い、はっきり伸ばす。
HOLD_L1 = {
    #        し  ー  ろ  い  い  ろ  が  す
    "white": [9, 9, 6, 6, 3, 6, 6, 3],
    #             み  ー  ず  い  ろ  が  す
    "lightblue": [9, 9, 6, 6, 6, 9, 3],
}

# 「ー」を置く音符の位置（1行目の何音目か、0始まり）。
# 直前の音と同じ高さにしないと母音の継続にならない。
HOLD_AT = {"white": 1, "lightblue": 1}


def apply_durations(measure, durs):
    notes = pitched(measure)
    assert len(notes) == len(durs), f"{len(notes)} notes vs {len(durs)} durations"
    assert sum(durs) == 48, f"合計 {sum(durs)} != 48"
    for n, d in zip(notes, durs):
        set_duration(n, d)


def unify_pitch(measure, i):
    """i 番目の音を直前の音と同じ高さに揃える（母音を伸ばすため）。"""
    notes = pitched(measure)
    prev = notes[i - 1].find("pitch")
    cur = notes[i]
    cur.remove(cur.find("pitch"))
    cur.insert(0, copy.deepcopy(prev))


def main():
    root, measures = load()
    made = []

    def emit(name, spans, fifths, mods=None):
        out = build(root, measures, name, spans, fifths, mods)
        made.append((name, len(spans), write(out, name)))
        return out

    # --- 構造パート ---
    emit("intro", rng(1, 3), 1)

    # 質問。m5 の3-4拍目にある合いの手（あか！）は消す。
    # 伴奏は残すので、子どもはその上に叫ぶことになる。
    emit("question", rng(4, 5), 1,
         lambda pid, num, m: mute_melody(m, 2.0) if pid == "P1" and num == "5" else None)

    emit("bridge", rng(40, 47), 1)

    # 間奏。m27 の頭にある「ン」は前の節のものなので落とし、
    # 3拍目裏からの助走だけを残す。
    emit("interlude", rng(27, 31), 1,
         lambda pid, num, m: mute_before(m, 2.0) if pid == "P1" and num == "27" else None)

    # ぜんぶ＋エンディング。
    #
    # 転調のつなぎとして m49 を頭に置いていたが、m49 のピアノは
    # 1拍目が休符で、しかも m50 と完全に同一のヴァンプだった。
    # 無音の1拍と冗長な1小節が増えるだけなので落とす。
    # イ長調の節がいきなり頭から鳴るほうが、半音上がりが決まる。
    # 末尾の m58 は両パートとも完全に空の小節なので落とす。
    emit("all", rng(50, 57), 3)

    # --- 原曲にある4色 ---
    for name, spans in ORIGINAL.items():
        # きいろの m27 末尾にある助走は間奏側のものなので落とす
        mods = None
        if name == "yellow":
            mods = lambda pid, num, m: (
                mute_melody(m, 2.0) if pid == "P1" and num == "27" else None)
        emit(name, spans, 1, mods)

    # --- 原曲にない8色（あかの節を型として編曲） ---
    for name, (l1, l3) in NEW.items():
        out = build(root, measures, name, ORIGINAL["red"], 1)
        mm = out.find("part[@id='P1']").findall("measure")
        refit_verse(mm, l1, l3)
        if name in HOLD_L1:
            apply_durations(mm[0], HOLD_L1[name])
            unify_pitch(mm[0], HOLD_AT[name])
        made.append((name, 6, write(out, name)))

    for name, n, path in made:
        print(f"{name:<12} {n:>2}小節  {path}")
    print(f"\n計 {len(made)} ファイル / BPM {BPM}")


if __name__ == "__main__":
    main()
