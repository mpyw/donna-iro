//! 認識結果の文字列から応答を判定する。
//!
//! **信用できるのは先頭だけ。** tiny は短い音を渡されるとデコーダが
//! 暴走して、言っていない語を後ろに継ぎ足すことがある。しかも同じ語の
//! 繰り返しとは限らず、別の色が混ざる。だから後ろは捨てる。
//!
//! **位置が最優先。** 先頭の区間から順に、それぞれで次を試す。
//!
//! 1. 完全一致
//! 2. 部分一致（長い読みから順に）
//! 3. 音の近さで重み付けした編集距離
//!
//! 段の優先度を位置より上に置くと、後ろの区間が完全一致しただけで
//! 先頭を追い越してしまう。「みじろ、きいろ」で実際にそうなった。
//! 先頭は3段目でしか当たらないが、それでも先頭を採るべき。
//!
//! 当たらなければ次の区間と繋げて同じことを試す。認識が語を割ることが
//! あるため（「ちゃいろ」が「じゃあ、いろ」になった）。
//!
//! どれも該当しなければ `None`。呼び出し側でランダムな色に倒す。

use sort_const::const_quicksort;

use crate::app::color::Answer;
use const_for::const_for;

/// 質問の歌がマイクに回り込んだぶんを落とす。
///
/// 「どんないろがすき」には「いろ」が含まれるので、残したまま判定すると
/// きいろの読み「いーろ」に化けることがある。長いものから順に削る。
const PROMPT: &[&str] = &["どんないろがすき", "どんないろ", "どんな"];

/// 質問文そのもの。**区間まるごとがこの一部なら、答えではなく回り込み。**
///
/// `PROMPT` を削るだけでは足りない。whisper が「どんな いろ」と空白で
/// 割って返すと、区切ってから削るので破片の「いろ」が生き残り、音の
/// 近さで「しろ」に当たる。**しかも位置を最優先するので、後ろに続く
/// 本当の答えを差し置いて毎回それが鳴る。**
///
/// 色の読みはどれもこの文の一部ではないので、丸ごと一致で捨ててよい。
/// 破れていないことは `no_answer_hides_inside_the_question` で見る。
const QUESTION: &str = "どんないろがすき";

/// 拗音と長音を落とした骨格。編集距離を測るときだけ使う。
///
/// 幼児の発音は「ぜ」が「じぇ」に、「ぶ」が「ば」に寄る。小書き文字と
/// 長音符が入ると字数がずれて、素の編集距離では届かなくなる。
/// 落としてから比べると、残るのは子音・母音のずれだけになる。
///
/// 読みの側も同じ関数を通すので、「ちゃいろ」が「ちいろ」に潰れても
/// 両側が揃っていて問題ない。
fn skeleton(s: &str) -> Vec<char> {
    s.chars()
        .filter(|c| {
            !matches!(
                c,
                'ー' | 'ぁ' | 'ぃ' | 'ぅ' | 'ぇ' | 'ぉ' | 'ゃ' | 'ゅ' | 'ょ' | 'ゎ' | 'っ'
            )
        })
        .collect()
}

/// 漢字表記を持つ応答の数。候補の丈を出すためだけに数える。
const fn kanji_count() -> usize {
    let every = Answer::every();
    let mut n = 0;
    const_for!(i in 0..Answer::COUNT => {
        if every[i].kanji().is_some() {
            n += 1;
        }
    });
    n
}

/// UTF-8 の文字数。`str::chars` は const で回せないので先頭バイトを数える。
const fn char_count(s: &str) -> usize {
    let bytes = s.as_bytes();
    let mut n = 0;
    const_for!(i in 0..bytes.len() => {
        // 継続バイト（10xxxxxx）以外が1文字の先頭。
        if bytes[i] & 0xC0 != 0x80 {
            n += 1;
        }
    });
    n
}

/// 候補の数。読みが全応答ぶん、漢字はある応答のぶんだけ。
const CANDIDATE_COUNT: usize = Answer::COUNT + kanji_count();

/// 判定の候補。
///
/// **`normalize` は通さない。** 語彙の側が正規形で書かれている約束
/// （`color.rs` を見ること）なので、通しても何も起きない。破れていない
/// ことは `vocabulary_is_already_normalized` で見ている。
const CANDIDATES: [(Answer, &str); CANDIDATE_COUNT] = {
    // 候補の並び順。**同点が出ないところまで見る。**
    //
    // 長さの降順が主。短い色名が長い色名に含まれる組（「きみどり」⊃「みどり」）が
    // あるので、部分一致は長いほうから試さないと取りこぼす。
    //
    // 同じ長さなら元の位置の昇順。作る順は「読みを全部 → 漢字を全部」で、
    // どちらも `Answer::every()` の順なので、これで「読みが漢字より先」と
    // 「先に宣言した色が先」の両方が決まる。
    //
    // **ここまで見れば同点が無い。** 並べ替えが安定かどうかに結果が依らない
    // ので、不安定なアルゴリズムを使ってもよい。
    const fn before(a: (Answer, &str, usize), b: (Answer, &str, usize)) -> bool {
        let (la, lb) = (char_count(a.1), char_count(b.1));
        if la != lb {
            la > lb
        } else {
            a.2 < b.2
        }
    }

    let every = Answer::every();

    // 3つ目は作った順。並べ替えの second key に使う。
    let mut buf = [(Answer::All, "", 0usize); CANDIDATE_COUNT];
    let mut n = 0;
    const_for!(i in 0..Answer::COUNT => {
        buf[n] = (every[i], every[i].reading(), n);
        n += 1;
    });
    const_for!(i in 0..Answer::COUNT => {
        if let Some(k) = every[i].kanji() {
            buf[n] = (every[i], k, n);
            n += 1;
        }
    });

    // 配列を渡すと**その場では並べ替えず、並べ替えたものを返す**。
    // 戻り値を捨てると黙って未整列のまま進むので、必ず受け直すこと。
    let buf = const_quicksort!(buf, |a, b| before(*a, *b));

    // 並べ終わったら位置は要らない。
    let mut out = [(Answer::All, ""); CANDIDATE_COUNT];
    const_for!(i in 0..CANDIDATE_COUNT => {
        out[i] = (buf[i].0, buf[i].1);
    });
    out
};

pub struct Matcher {
    /// 候補の骨格。編集距離用。並びは `CANDIDATES` と同じ。
    ///
    /// `skeleton` が `Vec<char>` を返すので、ここだけは実行時に作る。
    skeletons: Vec<(Answer, Vec<char>)>,
    /// 先頭から信用する区間の数。tiny は言っていない語を後ろに継ぎ足す。
    head: usize,
}

impl Matcher {
    pub fn new(head: usize) -> Self {
        Self {
            // **骨格は読みだけ。** 漢字は `decompose` できず全文字が
            // 満額の置換になるので、音の近さの土俵に乗らない。無駄な
            // 距離計算になるうえ、同点（→ 諦める）の芽にもなる。
            // 漢字は完全一致と部分一致の2段で拾えている。
            skeletons: CANDIDATES
                .iter()
                .filter(|(a, r)| a.kanji() != Some(*r))
                .map(|(a, r)| (*a, skeleton(r)))
                .collect(),
            head: head.max(1),
        }
    }

    pub fn find(&self, raw: &str) -> Option<Answer> {
        let mut parts: Vec<String> = Vec::new();
        // 直前の区間へ繋いでよいか。**質問が挟まった時点で切れる。**
        let mut joinable = false;
        // まだ実のある区間が1つも無いときの溜め。**破片は後ろ向きにしか
        // 繋げないので、割れた語の1つ目が質問文の一部だと繋ぎ先が無い。**
        // 「き、いろ」の「き」は「すき」の一部なので、そのまま捨てると
        // きいろ が丸ごと消える。
        let mut pending = String::new();

        for seg in segments(&normalize(raw)) {
            if seg.is_empty() {
                // 区切りが続いただけ。何も起きていない。
                continue;
            }
            let seg = strip_prompt(&seg);
            if seg.is_empty() {
                // まるごと質問だった。**ここから後ろの破片は、前の語の
                // 続きではなく質問の続き。** 繋ぎ先を切り、溜めも捨てる。
                joinable = false;
                pending.clear();
                continue;
            }
            if QUESTION.contains(seg.as_str()) {
                // 質問文の破片。**単独では答えにならない。** ただし認識が
                // 語を割っただけということもある（「ちゃいろ」→「じゃあ、
                // いろ」）ので、直前が実のある区間ならそちらへ繋ぐ。
                //
                // 繋ぎ先が切れていれば捨てる。切らずにいると
                // 「じゃあ どんな いろ あか」の「いろ」が「じゃあ」に
                // 付いて「ちゃいろ」になり、また「あか」を押し出す。
                //
                // **捨てるのは `truncate` の前でなければならない。** 破片が
                // 席を占めると、後ろの本当の答えが `head` から押し出される。
                if parts.is_empty() {
                    // 繋ぎ先がまだ無い。溜めておいて、**質問文の一部で
                    // なくなった時点で**実のある区間に昇格させる。
                    // 「き」→「きいろ」で抜ける。
                    //
                    // 質問がそのまま割れて届くぶんには、順番どおり
                    // 「いろ」「いろが」「いろがすき」と伸びるので
                    // 一度も抜けない。逆順に届けば抜けてしまうが、
                    // 歌が逆順に回り込むことはない。
                    pending.push_str(&seg);
                    if !QUESTION.contains(pending.as_str()) {
                        parts.push(std::mem::take(&mut pending));
                        joinable = true;
                    }
                    continue;
                }
                if joinable {
                    if let Some(prev) = parts.last_mut() {
                        prev.push_str(&seg);
                    }
                }
                continue;
            }
            parts.push(seg);
            joinable = true;
            pending.clear();
        }
        // 同じ語の繰り返しは畳む。tiny が出力を繰り返すことがある。
        parts.dedup();
        parts.truncate(self.head);

        // 先頭の区間から順に。単独で当たらなければ次と繋げて試す。
        //
        // **結合の完全一致は、単独の音の近さより先に見る。** 単独で3段を
        // 使い切ってから結合に移ると、割れた語の前半がたまたま別の色に
        // 近いだけで確定してしまう。
        //
        //     「むら さき」→ くろ    （むら が d=4 で当たる）
        //     「きみ どり」→ きいろ  （きみ が d=6 で当たる）
        //
        // 空白が区切りになる前は `normalize` が繋ぎ直していたので、この段の
        // 脆さは読点割れでしか出なかった。
        //
        // **結合の部分一致は単独の音の近さより後ろ。** 前に出すと
        // 「みじろ、きいろ」が結合の部分一致で きいろ になり、幻覚の
        // 継ぎ足しを捨てる仕掛けが壊れる。
        (0..parts.len()).find_map(|i| {
            let single = parts[i].as_str();
            let joined = (parts.len() - i >= 2).then(|| parts[i..i + 2].concat());
            let joined = joined.as_deref();
            self.exact(single)
                .or_else(|| self.substring(single))
                .or_else(|| joined.and_then(|j| self.exact(j)))
                .or_else(|| self.nearest(single))
                .or_else(|| joined.and_then(|j| self.substring(j)))
                .or_else(|| joined.and_then(|j| self.nearest(j)))
        })
    }

    fn exact(&self, text: &str) -> Option<Answer> {
        CANDIDATES.iter().find(|(_, r)| *r == text).map(|(a, _)| *a)
    }

    fn substring(&self, text: &str) -> Option<Answer> {
        CANDIDATES
            .iter()
            .find(|(_, r)| text.contains(*r))
            .map(|(a, _)| *a)
    }

    fn nearest(&self, text: &str) -> Option<Answer> {
        // 許容する編集距離。短い読みほど厳しくする。
        //
        // 単位は `substitution` の重みなので、4 が「1音ぶんのずれ」にあたる。
        // 2文字の読みは1音、3文字以上は1音半まで許す。
        //
        // **「ぜんぶ」だけは1音までに絞る。** 誤検出するとゲームが終わって
        // しまうため。緩めると「でんわ」あたりが引っかかる。
        // 逆に取りこぼしても次の周回でまた聞けるので、そちらの害は小さい。
        const fn allowed(answer: Answer, skeleton_len: usize) -> usize {
            match answer {
                Answer::All => 4,
                Answer::Single(_) if skeleton_len <= 2 => 4,
                Answer::Single(_) => 6,
            }
        }

        let chars = skeleton(text);
        // **1音では土俵に乗らない。** どの読みも骨格で2文字以上あるので、
        // 1文字が当たったとしてもそれは「近い」のではなく「短すぎて何にでも
        // 近い」。「えー」が「あお」に化けて、位置優先で後ろの本当の答えを
        // 押し出していた。
        if chars.len() < 2 {
            return None;
        }
        let mut scores: Vec<(Answer, usize)> = Vec::new();
        for (a, r) in self.skeletons.iter() {
            let d = levenshtein(&chars, r);
            if d > allowed(*a, r.len()) {
                continue;
            }
            scores.push((*a, d));
        }
        scores.sort_by_key(|&(_, d)| d);

        let (best, best_d) = *scores.first()?;
        // 同点なら諦める。「あか」と「あお」、「しろ」と「くろ」のように
        // 1文字違いの色があるので、割れたまま採用すると取り違える。
        if scores.get(1).is_some_and(|&(_, d)| d == best_d) {
            return None;
        }
        Some(best)
    }
}

/// 句読点で区切る。
///
/// whisper が不明瞭な音を受け取ると、`initial_prompt` に渡した語彙を
/// そのまま並べて返すことがある。実際に「あお、きいろ、みどり、しろ、みど」
/// が出た。区切らずに繋げると、部分一致が長い読みから探すせいで
/// 途中の「きいろ」を拾ってしまう。区切って先頭から見れば、
/// 最初に言われた色を優先できる。
///
/// **ひらがな以外を境界にしてはいけない。** 漢字が区間から消えるので、
/// 「全部」がまるごと落ちる。実際にそうなった。
fn segments(s: &str) -> Vec<String> {
    s.split(is_separator)
        .filter(|seg| !seg.is_empty())
        .map(str::to_string)
        .collect()
}

fn is_separator(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '、' | '。'
                | '，'
                | '．'
                | ','
                | '.'
                | '!'
                | '?'
                | '！'
                | '？'
                | '「'
                | '」'
                | '（'
                | '）'
                | '('
                | ')'
                | '・'
                | '〜'
                | '~'
                | '/'
        )
}

/// カタカナをひらがなに寄せる。
///
/// **通すのは認識結果だけ。** 語彙の側は初めから正規形で書く約束なので、
/// カタカナ表記を並べる必要も、ここを通す必要もない。
/// 長音符「ー」はカタカナ領域の外なのでそのまま残る。
///
/// **空白も句読点も落とさない。** どちらも `segments` が区切りとして使う。
/// ここで空白を消していた頃は `is_separator` の空白判定が到達せず、
/// 「あお あか」が「あおあか」に繋がって、部分一致で後ろの色が勝っていた。
/// 先頭を優先する方針が空白入力で破れる。
fn normalize(s: &str) -> String {
    s.chars()
        .map(|c| {
            let u = c as u32;
            if (0x30A1..=0x30F6).contains(&u) {
                char::from_u32(u - 0x60).unwrap_or(c)
            } else {
                c
            }
        })
        .collect()
}

fn strip_prompt(s: &str) -> String {
    let mut out = s.to_string();
    for p in PROMPT {
        out = out.replace(p, "");
    }
    out
}

/// 五十音表。行の子音と、あいうえお順の並び。
/// `_` はその行に無い音。
const ROWS: &[(u8, &str)] = &[
    (b'-', "あいうえお"),
    (b'k', "かきくけこ"),
    (b's', "さしすせそ"),
    (b't', "たちつてと"),
    (b'n', "なにぬねの"),
    (b'h', "はひふへほ"),
    (b'm', "まみむめも"),
    (b'y', "や_ゆ_よ"),
    (b'r', "らりるれろ"),
    (b'w', "わ___を"),
    (b'g', "がぎぐげご"),
    (b'z', "ざじずぜぞ"),
    (b'd', "だぢづでど"),
    (b'b', "ばびぶべぼ"),
    (b'p', "ぱぴぷぺぽ"),
];

/// 仮名を（子音, 母音）に分ける。表に無ければ `None`。
fn decompose(c: char) -> Option<(u8, usize)> {
    ROWS.iter().find_map(|(consonant, row)| {
        row.chars()
            .position(|k| k == c)
            .map(|vowel| (*consonant, vowel))
    })
}

/// 混同しやすい子音の組。幼児の発音でも音声認識でも入れ替わる。
///
/// `sztd` をひとまとめにしているのは、い段で「し・じ・ち・ぢ」が
/// 硬口蓋の摩擦音／破擦音に寄って区別が付かなくなるため。
/// 実際に「ちゃいろ」が「じゃあ、いろ」と認識された。
const NEAR_CONSONANTS: &[&[u8]] = &[
    b"kg",   // か行・が行（清濁）
    b"sztd", // さ・ざ・た・だ行（清濁と、し/じ/ち/ぢ の混同）
    b"hbp",  // は・ば・ぱ行
    b"mn",   // 鼻音
    b"rd",   // ら行・だ行（幼児で入れ替わる）
];

/// 母音だけの音（あいうえお）を表す印。
const NO_CONSONANT: u8 = b'-';

fn consonant_cost(a: u8, b: u8) -> usize {
    if a == b {
        0
    } else if NEAR_CONSONANTS
        .iter()
        .any(|g| g.contains(&a) && g.contains(&b))
    {
        1
    } else if a == NO_CONSONANT || b == NO_CONSONANT {
        // 子音が丸ごと消える／生えるのは、別の子音に変わるより大きい変化。
        // ここを同じ扱いにすると「みじろ」が「みずいろ」ではなく
        // 「きいろ」に寄ってしまう（じ→い を安く見積もるため）。
        3
    } else {
        2
    }
}

/// 挿入・削除の重み。
///
/// 母音だけの音は落ちたり伸びたりしやすいので安くする。
/// 「みずいろ」が「みじろ」と認識されたように、幼児の発音でも
/// 認識結果でも、母音1つの脱落は頻繁に起きる。
fn indel(c: char) -> usize {
    match decompose(c) {
        Some((NO_CONSONANT, _)) => 2,
        _ => INDEL,
    }
}

/// 置換の重み。1音ぶんのずれが 4 になる目盛り。
///
/// 素の編集距離だと文字が違えば一律1で、「じ→ぜ」（同じザ行）と
/// 「ば→く」（無関係）が同じ扱いになる。子音と母音を別々に数え、
/// さらに子音は音の近さで刻む。
///   じ→ぜ  子音 z=z(0)      母音 i≠e(2)  → 2
///   じ→ち  子音 z≈t(1)      母音 i=i(0)  → 1
///   じ→き  子音 z≠k(2)      母音 i=i(0)  → 2
///   じ→い  子音 z→無し(3)   母音 i=i(0)  → 3
///   ば→く  子音 b≠k(2)      母音 a≠u(2)  → 4
fn substitution(a: char, b: char) -> usize {
    if a == b {
        return 0;
    }
    match (decompose(a), decompose(b)) {
        (Some((ca, va)), Some((cb, vb))) => consonant_cost(ca, cb) + if va == vb { 0 } else { 2 },
        // 「ん」や表に無い文字。似ている度合いを測れないので別物とみなす。
        _ => 4,
    }
}

/// 挿入・削除の重み。1音まるごとのずれを 4 とする目盛りに合わせる。
/// 置換の最大は 5（子音の消失3＋母音違い2）なので、そちらのほうが
/// わずかに高い。つまり同じ長さなら置換より挿入＋削除を選びにくい。
///
/// もとの意図は、
/// 「1音ぶんのずれ」がどの操作でも同じ値になるようにする。
const INDEL: usize = 4;

/// 音の近さで重み付けした編集距離。日本語なので文字単位で測る。
fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = std::iter::once(0)
        .chain(b.iter().scan(0, |acc, &c| {
            *acc += indel(c);
            Some(*acc)
        }))
        .collect();
    let mut cur: Vec<usize> = vec![0; b.len() + 1];
    for ca in a {
        cur[0] = prev[0] + indel(*ca);
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j + 1] + indel(*ca))
                .min(cur[j] + indel(*cb))
                .min(prev[j] + substitution(*ca, *cb));
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use crate::app::color::Color;

    use super::*;

    pub fn find(s: &str) -> Option<Answer> {
        Matcher::new(2).find(s)
    }
    fn c(x: Color) -> Option<Answer> {
        Some(Answer::Single(x))
    }

    /// 候補を `normalize` に通していないので、**語彙の側が正規形である
    /// ことが前提**になる。カタカナや空白が紛れ込むと、判定が黙って
    /// 当たらなくなる。制約が破れたらここで気づく。
    #[test]
    fn vocabulary_is_already_normalized() {
        for a in Answer::every() {
            assert_eq!(normalize(a.reading()), a.reading(), "読みが正規形でない");
            if let Some(k) = a.kanji() {
                assert_eq!(normalize(k), k, "漢字表記が正規形でない");
            }
        }
    }

    /// 部分一致は長いほうから試す。並びが崩れると「きみどりがすき」が
    /// 「みどり」に化ける。const で組んであるので、崩れたらここで気づく。
    #[test]
    fn candidates_are_sorted_longest_first() {
        let lens: Vec<usize> = CANDIDATES.iter().map(|(_, r)| r.chars().count()).collect();
        assert!(
            lens.windows(2).all(|w| w[0] >= w[1]),
            "長い順でない: {lens:?}"
        );

        // 同じ長さなら読みが先。`after` が元の位置まで見るので、並べ替えの
        // 安定性に関係なくこの順になる。ここで見ているのは「意図した順に
        // なっているか」であって、アルゴリズムの性質ではない。
        //
        // 崩れると「あかあお」のように繋がって認識されたとき、部分一致が
        // どちらを採るかが変わる。
        let is_kanji = |(a, r): &(Answer, &str)| a.kanji() == Some(*r);
        for w in CANDIDATES.windows(2) {
            if w[0].1.chars().count() == w[1].1.chars().count() && is_kanji(&w[0]) {
                assert!(is_kanji(&w[1]), "同じ長さで漢字のあとに読みが来た");
            }
        }

        // 同じ長さの読みどうしは宣言順。先に宣言した色が先に当たる。
        let two: Vec<&str> = CANDIDATES
            .iter()
            .filter(|c| c.1.chars().count() == 2 && !is_kanji(c))
            .map(|c| c.1)
            .collect();
        assert_eq!(two, ["あか", "あお", "しろ", "くろ"], "宣言順が崩れた");

        // const で数えた丈が、実際に数えたものと合っているか。
        let kanji = Answer::every()
            .iter()
            .filter(|a| a.kanji().is_some())
            .count();
        assert_eq!(CANDIDATES.len(), Answer::COUNT + kanji);
        assert_eq!(char_count("きみどり"), "きみどり".chars().count());
    }

    #[test]
    fn exact_match() {
        assert_eq!(find("あか"), c(Color::Red));
        assert_eq!(find("みずいろ"), c(Color::LightBlue));
        assert_eq!(find("ぜんぶ"), Some(Answer::All));
    }

    #[test]
    fn katakana_is_normalized_to_hiragana() {
        assert_eq!(find("アカ"), c(Color::Red));
        assert_eq!(find("ピンク"), c(Color::Pink));
        assert_eq!(find("オレンジ"), c(Color::Orange));
    }

    #[test]
    fn punctuation_and_space_are_stripped() {
        assert_eq!(find(" あか！ "), c(Color::Red));
        assert_eq!(find("あお。"), c(Color::Blue));
    }

    #[test]
    fn substring_prefers_the_longest_reading() {
        // 「きみどり」は「みどり」を含む。短いほうから見ると取りこぼす。
        assert_eq!(find("きみどりがすき"), c(Color::YellowGreen));
        assert_eq!(find("みどりだよ"), c(Color::Green));
    }

    #[test]
    fn fuzzy_match_absorbs_mispronunciation() {
        assert_eq!(find("むらさぎ"), c(Color::Purple));
        assert_eq!(find("みずいる"), c(Color::LightBlue));
    }

    #[test]
    fn closest_sounding_color_wins() {
        // 「あき」は「あか」（カ行の母音違い）のほうが
        // 「あお」（子音も母音も違う）より近い。
        assert_eq!(find("あき"), c(Color::Red));
    }

    #[test]
    fn all_absorbs_small_slips() {
        assert_eq!(find("ぜんぷ"), Some(Answer::All));
        assert_eq!(find("ぜんむ"), Some(Answer::All));
        // 実際に出た形。拗音と長音を落とすと「じんば」で距離2。
        assert_eq!(find("ジェンバー"), Some(Answer::All));
        assert_eq!(find("じゃんぶー"), Some(Answer::All));
    }

    #[test]
    fn all_does_not_fire_on_distant_words() {
        // 音が2つずれるものまで拾うとゲームが事故で終わる。
        assert_ne!(find("でんわ"), Some(Answer::All));
        assert_ne!(find("わんわん"), Some(Answer::All));
        assert_ne!(find("ごはん"), Some(Answer::All));
    }

    #[test]
    fn empty_input() {
        assert_eq!(find(""), None);
        assert_eq!(find("   "), None);
    }

    #[test]
    fn unrelated_speech_is_rejected() {
        assert_eq!(find("おかあさん"), None);
    }

    #[test]
    fn kanji_is_matched() {
        // initial_prompt は誘導であって強制ではないので漢字は出る。
        assert_eq!(find("全部"), Some(Answer::All));
        assert_eq!(find("赤"), c(Color::Red));
        assert_eq!(find("黄色"), c(Color::Yellow));
        assert_eq!(find("黄緑"), c(Color::YellowGreen));
        assert_eq!(find("水色"), c(Color::LightBlue));
    }

    #[test]
    fn prompt_echo_takes_the_first_color() {
        // whisper が initial_prompt の語彙を吐き返したときに実際に出た形。
        // 繋げて探すと途中の「きいろ」を拾ってしまう。
        assert_eq!(find("あお、きいろ、みどり、しろ、みど"), c(Color::Blue));
    }

    #[test]
    fn split_word_is_rejoined() {
        // 認識が「ちゃいろ」を「じゃあ、いろ」に割ったときの実例。
        // 後半だけ見ると「しろ」が最も近い。全体で見れば茶色が勝つ。
        assert_eq!(find("じゃあ、いろ。"), c(Color::Brown));

        // **前半がたまたま別の色に近いだけで確定してはいけない。**
        // 単独で3段を使い切ってから結合に移っていた頃は、「むら」が
        // くろ に、「きみ」が きいろ に音の近さで当たって、結合の
        // 完全一致まで届かなかった。
        assert_eq!(find("むら さき"), c(Color::Purple));
        assert_eq!(find("むら、さき"), c(Color::Purple));
        assert_eq!(find("きみ どり"), c(Color::YellowGreen));

        // **割れた語の1つ目が質問文の一部でも消してはいけない。**
        // 「き」は「すき」の一部。後ろ向きにしか繋がなかった頃は、
        // 繋ぎ先が無くて捨てられ、きいろ が丸ごと消えていた。
        assert_eq!(find("き、いろ"), c(Color::Yellow));
        assert_eq!(find("き いろ"), c(Color::Yellow));
        assert_eq!(find("みず いろ"), c(Color::LightBlue));
        assert_eq!(find("ちゃ いろ"), c(Color::Brown));

        // 幻覚の継ぎ足しを捨てる仕掛けは保つ。結合の部分一致を
        // 単独の音の近さより前に出すと、ここが きいろ になる。
        assert_eq!(find("みじろ、きいろ"), c(Color::LightBlue));
        assert_eq!(find("どんな いろ みじろ"), c(Color::LightBlue));
    }

    #[test]
    fn dropped_vowel_is_cheap() {
        // 「みずいろ」が「みじろ」と認識された実例。母音の脱落を
        // 高く見積もると「きいろ」に負ける。
        assert_eq!(find("みじろ"), c(Color::LightBlue));
        assert_eq!(find("みじろ、みじろ。"), c(Color::LightBlue));
    }

    #[test]
    fn hallucinated_tail_is_ignored() {
        // tiny は言っていない語を後ろに継ぎ足す。先頭を優先する。
        assert_eq!(find("あお、おれんじ、あか"), c(Color::Blue));
        assert_eq!(find("あか、あお、みどり、しろ、くろ"), c(Color::Red));
        // 先頭が3段目でしか当たらなくても、後ろの完全一致に譲らない。
        assert_eq!(find("みじろ、みじろ、きいろ、しろ"), c(Color::LightBlue));
        assert_eq!(find("みじろ、きいろ"), c(Color::LightBlue));
    }

    #[test]
    fn earlier_segment_wins() {
        assert_eq!(find("あか。あお"), c(Color::Red));
        assert_eq!(find("むらさき / みどり"), c(Color::Purple));
        // 空白も区切り。繋げてしまうと部分一致で後ろの色が勝つ。
        assert_eq!(find("あお あか"), c(Color::Blue));
        assert_eq!(find("みどり\tきいろ"), c(Color::Green));
    }

    #[test]
    fn question_bleeding_in_does_not_become_a_color() {
        // 質問が回り込んでも色にしない。「いろ」を残すときいろに化ける。
        assert_eq!(find("どんないろがすき"), None);
        // 質問の後ろに答えがくっついても答えだけ拾う。
        assert_eq!(find("どんないろがすきあか"), c(Color::Red));

        // **whisper は質問を空白で割って返す。** 区切ってから削るので、
        // 破片の「いろ」が生き残って「しろ」に化けていた。
        assert_eq!(find("どんな いろ"), None);
        assert_eq!(find("いろ"), None);
        assert_eq!(find("どんな いろ が すき"), None);
        // **そして位置を最優先するので、破片が本当の答えを押し出す。**
        // ランダムに外すのではなく、毎回きまって「しろ」が鳴っていた。
        assert_eq!(find("どんな いろ あか"), c(Color::Red));
        assert_eq!(find("どんな いろ が すき みどり"), c(Color::Green));

        // **前に何か挟まっても同じこと。** 破片を直前へ繋ぐようにしたら、
        // 今度は繋ぎ先がフィラーになって「ちゃいろ」「きみどり」に化けた。
        // 質問が挟まった時点で繋ぎ先を切る。
        assert_eq!(find("じゃあ どんな いろ あか"), c(Color::Red));
        assert_eq!(find("じゃあ どんな いろ が すき みどり"), c(Color::Green));
        assert_eq!(find("えー どんな いろ が すき みどり"), c(Color::Green));
    }

    /// 1音のフィラーは色にしない。
    ///
    /// **短すぎるものは何にでも近い。** 「えー」の骨格は「え」1文字で、
    /// 「あお」に一意に当たっていた。位置が最優先なので、後ろで本当に
    /// 言った色を押し出す。ランダムに外すのではなく毎回きまって外す。
    #[test]
    fn a_single_mora_is_not_a_color() {
        assert_eq!(find("えー"), None);
        assert_eq!(find("えー みどり"), c(Color::Green));
        assert_eq!(find("うーん あか"), c(Color::Red));
        // 下限の根拠。候補の骨格はどれも2文字以上ある。
        for (a, sk) in Matcher::new(2).skeletons.iter() {
            assert!(sk.len() >= 2, "{a:?} の骨格が2文字未満: {sk:?}");
        }
    }

    /// 質問文の破片を捨てる仕掛けが、答えのほうを巻き込んでいないこと。
    ///
    /// 色を足したときに「読みがたまたま質問文の一部だった」となると、
    /// その色だけ永久に当たらなくなる。無言に倒れるので気づきにくい。
    #[test]
    fn no_answer_hides_inside_the_question() {
        for (answer, reading) in CANDIDATES {
            assert!(
                !QUESTION.contains(reading),
                "{answer:?} の読み「{reading}」が質問文の一部になっている。\
                 このままだと聞き取れても捨てられる"
            );
        }
    }

    #[test]
    fn levenshtein_weighs_by_sound() {
        let d = |x: &str, y: &str| {
            let a: Vec<char> = x.chars().collect();
            let b: Vec<char> = y.chars().collect();
            levenshtein(&a, &b)
        };
        assert_eq!(d("あか", "あか"), 0);
        // 同じ行なら母音のずれだけ
        assert_eq!(d("じ", "ぜ"), 2);
        assert_eq!(d("ば", "ぶ"), 2);
        // 近い子音は安い
        assert_eq!(d("じ", "ち"), 1);
        assert_eq!(d("ぱ", "ば"), 1);
        // 子音が丸ごと消えるのは、別の子音に変わるより大きい
        assert!(d("じ", "い") > d("じ", "き"));
        // 子音も母音も違えば満額
        assert_eq!(d("ば", "く"), 4);
        // 「じんば」は「ぴんく」より「ぜんぶ」に近い
        assert!(d("じんば", "ぜんぶ") < d("じんば", "ぴんく"));
    }
}
