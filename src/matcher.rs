//! 認識結果の文字列から応答を判定する。
//!
//! まずひらがな以外で区切り、**先頭の区間から順に**判定する。
//! 各区間では次を順に試す。
//!
//! 1. 完全一致
//! 2. 部分一致（長い読みから順に）
//! 3. 編集距離が最小のもの
//!
//! どの区間も該当しなければ `None`。呼び出し側でランダムな色に倒す。

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

/// 質問の歌がマイクに回り込んだぶんを落とす。
///
/// 「どんないろがすき」には「いろ」が含まれるので、残したまま判定すると
/// きいろの読み「いーろ」に化けることがある。長いものから順に削る。
const PROMPT: &[&str] = &["どんないろがすき", "どんないろ", "どんな"];

/// 許容する編集距離。短い読みほど厳しくする。
///
/// 単位は `substitution` の重みなので、2 が「1音ぶんのずれ」にあたる。
/// 2文字の読みは1音、3文字以上は1音半まで許す。
///
/// **「ぜんぶ」だけは1音までに絞る。** 誤検出するとゲームが終わって
/// しまうため。3文字ぶん許すと「でんわ」あたりが引っかかる。
/// 逆に取りこぼしても次の周回でまた聞けるので、そちらの害は小さい。
fn allowed(answer: Answer, reading_len: usize) -> usize {
    match answer {
        Answer::All => 2,
        Answer::Color(_) if reading_len <= 2 => 2,
        Answer::Color(_) => 3,
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
        // 先頭の区間から順に見る。最初に言った色を優先するため。
        segments(&normalize(raw))
            .into_iter()
            .filter_map(|seg| {
                let text = strip_prompt(&seg);
                (!text.is_empty()).then(|| self.find_one(&text))?
            })
            .next()
    }

    fn find_one(&self, text: &str) -> Option<Answer> {
        // 1. 完全一致
        if let Some((a, _)) = self.candidates.iter().find(|(_, r)| r == text) {
            return Some(*a);
        }

        // 2. 部分一致
        if let Some((a, _)) = self
            .candidates
            .iter()
            .find(|(_, r)| text.contains(r.as_str()))
        {
            return Some(*a);
        }

        // 3. 編集距離。読みは色ごとに1つなので、ゆれはここで吸収する。
        //    拗音と長音を落とした骨格で比べる。
        let chars = skeleton(text);
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

/// 置換の重み。
///
/// 素の編集距離だと、文字が違えば一律1になる。それだと
/// 「じ→ぜ」（同じザ行）と「ば→く」（無関係）が同じ扱いになり、
/// 「じんば」が「ぜんぶ」と「ぴんく」の両方から等距離になってしまう。
///
/// 子音と母音を別々に数えると、音として近いほど安くなる。
///   じ→ぜ  子音 z=z、母音 i≠e        → 1
///   ば→ぶ  子音 b=b、母音 a≠u        → 1
///   ば→く  子音 b≠k、母音 a≠u        → 2
fn substitution(a: char, b: char) -> usize {
    if a == b {
        return 0;
    }
    match (decompose(a), decompose(b)) {
        (Some((ca, va)), Some((cb, vb))) => usize::from(ca != cb) + usize::from(va != vb),
        // 「ん」や表に無い文字。似ている度合いを測れないので別物とみなす。
        _ => 2,
    }
}

/// 挿入・削除の重み。置換の最大と揃えて、
/// 「1音ぶんのずれ」がどの操作でも同じ値になるようにする。
const INDEL: usize = 2;

/// 音の近さで重み付けした編集距離。日本語なので文字単位で測る。
fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).map(|i| i * INDEL).collect();
    let mut cur: Vec<usize> = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = (i + 1) * INDEL;
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j + 1] + INDEL)
                .min(cur[j] + INDEL)
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
        assert_eq!(d("じ", "ぜ"), 1);
        assert_eq!(d("ば", "ぶ"), 1);
        // 子音も母音も違えば2
        assert_eq!(d("ば", "く"), 2);
        // 「じんば」は「ぴんく」より「ぜんぶ」に近い
        assert!(d("じんば", "ぜんぶ") < d("じんば", "ぴんく"));
    }
}
