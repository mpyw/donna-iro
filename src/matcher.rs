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

use crate::color::Color;

/// 子どもの応答。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// 色を答えた。ループを続ける。
    Color(Color),
    /// 「ぜんぶ！」と答えた。転調してエンディングへ向かう。
    All,
}

/// 「ぜんぶ」の読み。ここにマッチしたらゲーム終了。
pub const ALL_READING: &str = "ぜんぶ";
/// 同じものの漢字表記。`initial_prompt` は誘導であって強制ではないので、
/// whisper は漢字を返すことがある。実際に「全部」が出た。
const ALL_KANJI: &str = "全部";

/// 先頭から信用する区間の数。
///
/// tiny は言っていない語を後ろに継ぎ足すので、全部を見ると
/// 混ざったものを拾ってしまう。かといって1区間に絞ると、
/// 「えーと、あか」のような言い淀みや、語が割れた場合を落とす。
const HEAD: usize = 2;

/// 質問の歌がマイクに回り込んだぶんを落とす。
///
/// 「どんないろがすき」には「いろ」が含まれるので、残したまま判定すると
/// きいろの読み「いーろ」に化けることがある。長いものから順に削る。
const PROMPT: &[&str] = &["どんないろがすき", "どんないろ", "どんな"];

/// 許容する編集距離。短い読みほど厳しくする。
///
/// 単位は `substitution` の重みなので、4 が「1音ぶんのずれ」にあたる。
/// 2文字の読みは1音、3文字以上は1音半まで許す。
///
/// **「ぜんぶ」だけは1音までに絞る。** 誤検出するとゲームが終わって
/// しまうため。緩めると「でんわ」あたりが引っかかる。
/// 逆に取りこぼしても次の周回でまた聞けるので、そちらの害は小さい。
fn allowed(answer: Answer, reading_len: usize) -> usize {
    match answer {
        Answer::All => 4,
        Answer::Color(_) if reading_len <= 2 => 4,
        Answer::Color(_) => 6,
    }
}

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

pub struct Matcher {
    /// 正規化済みの (応答, 読み)。長い読みから順に並べてある。
    candidates: Vec<(Answer, String)>,
    /// 同じものの骨格。編集距離用。
    skeletons: Vec<(Answer, Vec<char>)>,
}

impl Default for Matcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Matcher {
    pub fn new() -> Self {
        let mut candidates: Vec<(Answer, String)> = Vec::new();
        for c in Color::ALL {
            candidates.push((Answer::Color(c), normalize(c.reading())));
        }
        candidates.push((Answer::All, normalize(ALL_READING)));
        candidates.push((Answer::All, normalize(ALL_KANJI)));
        for c in Color::ALL {
            if let Some(k) = c.kanji() {
                candidates.push((Answer::Color(c), normalize(k)));
            }
        }
        // 短い色名が長い色名に含まれる組（「きみどり」⊃「みどり」）が
        // あるので、部分一致は長い読みから試さないと取りこぼす。
        candidates.sort_by_key(|(_, r)| std::cmp::Reverse(r.chars().count()));
        let skeletons = candidates.iter().map(|(a, r)| (*a, skeleton(r))).collect();
        Self {
            candidates,
            skeletons,
        }
    }

    pub fn find(&self, raw: &str) -> Option<Answer> {
        let mut parts: Vec<String> = segments(&normalize(raw))
            .into_iter()
            .map(|seg| strip_prompt(&seg))
            .filter(|seg| !seg.is_empty())
            .collect();
        // 同じ語の繰り返しは畳む。tiny が出力を繰り返すことがある。
        parts.dedup();
        parts.truncate(HEAD);

        // 先頭の区間から順に。単独で当たらなければ次と繋げて試す。
        (0..parts.len()).find_map(|i| {
            (1..=2.min(parts.len() - i)).find_map(|take| self.judge(&parts[i..i + take].concat()))
        })
    }

    fn judge(&self, text: &str) -> Option<Answer> {
        self.exact(text)
            .or_else(|| self.substring(text))
            .or_else(|| self.nearest(text))
    }

    fn exact(&self, text: &str) -> Option<Answer> {
        self.candidates
            .iter()
            .find(|(_, r)| r == text)
            .map(|(a, _)| *a)
    }

    fn substring(&self, text: &str) -> Option<Answer> {
        self.candidates
            .iter()
            .find(|(_, r)| text.contains(r.as_str()))
            .map(|(a, _)| *a)
    }

    fn nearest(&self, text: &str) -> Option<Answer> {
        let chars = skeleton(text);
        if chars.is_empty() {
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

/// カタカナをひらがなに寄せ、空白を落とす。
///
/// 認識結果と読みの両方を通すので、読みの側にカタカナ表記を並べる
/// 必要がない。長音符「ー」はカタカナ領域の外なのでそのまま残る。
/// 句読点は落とさない。`segments` が区切りとして使う。
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
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

/// 挿入・削除の重み。置換の最大と揃えて、
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
    for (i, ca) in a.iter().enumerate() {
        cur[0] = prev[0] + indel(*ca);
        let _ = i;
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
    use super::*;

    fn find(s: &str) -> Option<Answer> {
        Matcher::new().find(s)
    }
    fn c(x: Color) -> Option<Answer> {
        Some(Answer::Color(x))
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
    }

    #[test]
    fn question_bleeding_in_does_not_become_a_color() {
        // 質問が回り込んでも色にしない。「いろ」を残すときいろに化ける。
        assert_eq!(find("どんないろがすき"), None);
        // 質問の後ろに答えがくっついても答えだけ拾う。
        assert_eq!(find("どんないろがすきあか"), c(Color::Red));
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
