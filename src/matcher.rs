//! 認識結果の文字列から応答を判定する。
//!
//! 1. 完全一致
//! 2. 部分一致（長い読みから順に）
//! 3. 編集距離が最小のもの
//! 4. どれも該当しなければ `None`。呼び出し側でランダムな色に倒す

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
///
/// 実際に子どもの声を通したところ「じゃんぶー」「ジャンプ」に化けた。
/// 幼児の「ぜ」は摩擦が弱くて「じゃ」に寄りやすいので、その系統を拾う。
const ALL_READINGS: &[&str] = &[
    "ぜんぶ",
    "全部",
    "ぜーんぶ",
    "ぜんぶー",
    "ぜんぷ",
    "じゃんぶ",
    "じゃんぶー",
    "じゃんぷ",
    "じゃんぷー",
    "じぇんぶ",
    "でんぶ",
];

/// 質問の歌がマイクに回り込んだぶんを落とす。
///
/// 「どんないろがすき」には「いろ」が含まれるので、残したまま判定すると
/// きいろの読み「いーろ」に化けることがある。長いものから順に削る。
const PROMPT: &[&str] = &["どんないろがすき", "どんないろ", "どんな"];

/// 許容する編集距離。短い読みほど厳しくする。
/// 「あか」と「あお」は1文字違いなので、緩めると取り違える。
fn allowed(reading_len: usize) -> usize {
    if reading_len <= 4 {
        1
    } else {
        2
    }
}

pub struct Matcher {
    /// 正規化済みの (応答, 読み)。長い読みから順に並べてある。
    candidates: Vec<(Answer, String)>,
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
            for &r in c.readings() {
                candidates.push((Answer::Color(c), normalize(r)));
            }
        }
        for &r in ALL_READINGS {
            candidates.push((Answer::All, normalize(r)));
        }
        // 短い色名が長い色名に含まれる組（「きみどり」⊃「みどり」）が
        // あるので、部分一致は長い読みから試さないと取りこぼす。
        candidates.sort_by_key(|(_, r)| std::cmp::Reverse(r.chars().count()));
        Self { candidates }
    }

    pub fn find(&self, raw: &str) -> Option<Answer> {
        let text = strip_prompt(&normalize(raw));
        if text.is_empty() {
            return None;
        }

        // 1. 完全一致
        if let Some((a, _)) = self.candidates.iter().find(|(_, r)| *r == text) {
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

        // 3. 編集距離。
        //
        //    当初は「ぜんぶ」を除外していた。誤検出するとゲームが終わって
        //    しまうためだが、実際に試すと取りこぼしのほうが問題だった。
        //    「ぜんぶ」と紛らわしい色名は無いので、含めても事故は起きにくい。
        let chars: Vec<char> = text.chars().collect();
        let mut scores: Vec<(Answer, usize)> = Vec::new();
        for (a, r) in self.candidates.iter() {
            let rc: Vec<char> = r.chars().collect();
            let d = levenshtein(&chars, &rc);
            if d > allowed(rc.len()) {
                continue;
            }
            // 同じ色に複数の読みがあるので、その色での最小距離を持つ
            if let Some(slot) = scores.iter_mut().find(|s| s.0 == *a) {
                slot.1 = slot.1.min(d);
            } else {
                scores.push((*a, d));
            }
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

/// カタカナをひらがなに寄せ、空白と記号を落とす。
///
/// 認識結果と読みの両方を通すので、読みの側にカタカナ表記を並べる
/// 必要がない。長音符「ー」はカタカナ領域の外なのでそのまま残る。
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && !is_ignorable(*c))
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

fn is_ignorable(c: char) -> bool {
    matches!(
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
    )
}

fn strip_prompt(s: &str) -> String {
    let mut out = s.to_string();
    for p in PROMPT {
        out = out.replace(p, "");
    }
    out
}

/// 編集距離。日本語なのでバイトではなく文字単位で測る。
fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur: Vec<usize> = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
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
    fn kanji_readings() {
        assert_eq!(find("黄色"), c(Color::Yellow));
        assert_eq!(find("黄緑"), c(Color::YellowGreen));
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
    fn ambiguous_one_char_difference_is_rejected() {
        // 「あか」からも「あお」からも距離1。割れたまま採用すると取り違える。
        assert_eq!(find("あき"), None);
    }

    #[test]
    fn all_absorbs_toddler_pronunciation() {
        // 実際に化けた形。幼児の「ぜ」は「じゃ」に寄る。
        assert_eq!(find("じゃんぶー"), Some(Answer::All));
        assert_eq!(find("ジャンプ"), Some(Answer::All));
        assert_eq!(find("ぜんぷ"), Some(Answer::All));
        // 編集距離でも拾う
        assert_eq!(find("ぜんむ"), Some(Answer::All));
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
    fn question_bleeding_in_does_not_become_a_color() {
        // 質問が回り込んでも色にしない。「いろ」を残すときいろに化ける。
        assert_eq!(find("どんないろがすき"), None);
        // 質問の後ろに答えがくっついても答えだけ拾う。
        assert_eq!(find("どんないろがすきあか"), c(Color::Red));
    }

    #[test]
    fn levenshtein_basics() {
        let a: Vec<char> = "あか".chars().collect();
        let b: Vec<char> = "あお".chars().collect();
        assert_eq!(levenshtein(&a, &a), 0);
        assert_eq!(levenshtein(&a, &b), 1);
    }
}
