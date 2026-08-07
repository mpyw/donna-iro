//! 「どんな色がすき？」と呼びかけ、子どもが叫んだ色名を聞き取って
//! その色の続きを再生する。
//!
//! 設計方針は README を参照。要点は「無反応にしない」こと。
//! 色が判定できなくても必ず何かを再生する。
//!
//!     cargo run                                         マイク
//!     cargo run --no-default-features                   キーボード（進行の確認用）
//!     DONNA_IRO_ASSETS=assets/reference cargo run       合成音で試す

mod audio;
mod display;
mod listener;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;

use audio::Player;
use display::Rgb;
use listener::{Listener, Mic};

/// 認識対象の色。語彙をこれだけに閉じることで、
/// 汎用 ASR に頼らずとも判定できるようにする。
///
/// 歌がクレヨンの歌なので、標準的なクレヨン12色セット
/// （しろ・きいろ・きみどり・みどり・みずいろ・あお・むらさき・
/// ももいろ・あか・だいだい・ちゃいろ・くろ）に揃えてある。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Color {
    Red,
    Blue,
    Yellow,
    Green,
    YellowGreen,
    White,
    Black,
    Pink,
    Orange,
    Purple,
    Brown,
    LightBlue,
}

impl Color {
    const ALL: [Color; 12] = [
        Color::Red,
        Color::Blue,
        Color::Yellow,
        Color::Green,
        Color::YellowGreen,
        Color::White,
        Color::Black,
        Color::Pink,
        Color::Orange,
        Color::Purple,
        Color::Brown,
        Color::LightBlue,
    ];

    /// 認識結果の文字列にマッチさせる読み。表記ゆれと幼児の発音ゆれを吸収する。
    ///
    /// **カタカナ表記は並べない。** 判定前に認識結果も読みも `normalize()` で
    /// ひらがなに寄せるので、「ピンク」は「ぴんく」で拾える。漢字は
    /// ひらがなに変換できないので読みとして残す。
    ///
    /// 短い色名が長い色名に含まれる組（「きみどり」⊃「みどり」）があるため、
    /// 部分一致は長い読みから順に試すこと。`match_answer()` がそうしている。
    ///
    /// 「ももいろ」はピンク、「だいだい」はオレンジの読みとして扱う。
    /// クレヨンの表記と子どもが言う語が違うため、両方拾えるようにしてある。
    fn readings(&self) -> &'static [&'static str] {
        match self {
            Color::Red => &["あか", "赤"],
            Color::Blue => &["あお", "青"],
            // 「いーろ」は頭の「き」が落ちた発音。2歳児だと出にくい音なので拾う。
            // ただし裸の「いろ」は入れてはいけない。ほぼ全ての色名と
            // 歌詞そのものに含まれるので、何を言っても黄色になってしまう。
            Color::Yellow => &["きいろ", "黄色", "きーろ", "きいく", "いーろ"],
            Color::Green => &["みどり", "緑", "みろり"],
            Color::YellowGreen => &["きみどり", "黄緑", "きみろり"],
            Color::White => &["しろ", "白"],
            Color::Black => &["くろ", "黒"],
            Color::Pink => &["ぴんく", "ぴんこ", "ももいろ", "桃色"],
            Color::Orange => &["おれんじ", "おえんじ", "だいだい", "橙"],
            Color::Purple => &["むらさき", "紫", "むあさき"],
            Color::Brown => &["ちゃいろ", "茶色"],
            Color::LightBlue => &["みずいろ", "水色"],
        }
    }

    /// ターミナルに描く●の色。クレヨン12色の実際の色味に寄せてある。
    fn rgb(&self) -> Rgb {
        match self {
            Color::Red => (230, 0, 18),
            Color::Blue => (0, 104, 183),
            Color::Yellow => (255, 241, 0),
            Color::Green => (0, 153, 68),
            Color::YellowGreen => (143, 195, 31),
            Color::White => (245, 245, 245),
            Color::Black => (35, 24, 21),
            Color::Pink => (233, 84, 140),
            Color::Orange => (243, 152, 0),
            Color::Purple => (146, 7, 131),
            Color::Brown => (122, 69, 26),
            Color::LightBlue => (0, 160, 233),
        }
    }

    /// 音源のファイル名（拡張子なし）。
    fn stem(&self) -> &'static str {
        match self {
            Color::Red => "red",
            Color::Blue => "blue",
            Color::Yellow => "yellow",
            Color::Green => "green",
            Color::YellowGreen => "yellowgreen",
            Color::White => "white",
            Color::Black => "black",
            Color::Pink => "pink",
            Color::Orange => "orange",
            Color::Purple => "purple",
            Color::Brown => "brown",
            Color::LightBlue => "lightblue",
        }
    }
}

/// 音源のディレクトリ。つくよみちゃんの音源が揃うまでは
/// 確認用の合成音で試せる。
///
///     DONNA_IRO_ASSETS=assets/reference cargo run
fn asset(stem: &str) -> PathBuf {
    let dir = std::env::var("DONNA_IRO_ASSETS").unwrap_or_else(|_| "assets".to_string());
    PathBuf::from(dir).join(format!("{stem}.wav"))
}

/// 子どもの応答。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    /// 色を答えた。ループを続ける。
    Color(Color),
    /// 「ぜんぶ！」と答えた。転調してエンディングへ向かう。
    All,
}

/// 応答を待つ最大時間。2歳児は考えてから言うので短すぎると取りこぼす。
const LISTEN_MAX: Duration = Duration::from_secs(5);

/// 何周に1回、区切り（ブリッジまたは間奏）を挟むか。
const INSERT_EVERY: u32 = 3;

fn main() -> Result<()> {
    // 色味の確認用。音源もマイクも要らない。
    //     cargo run --no-default-features -- --palette
    if std::env::args().any(|a| a == "--palette") {
        draw_all();
        for c in Color::ALL {
            println!("{:?}", c);
            draw_one(c);
        }
        return Ok(());
    }

    let player = Player::new()?;
    let mut ears = if cfg!(feature = "whisper") {
        Listener::Mic(Mic::new()?)
    } else {
        Listener::Keyboard
    };

    // イントロは最初に一度だけ。
    draw_all();
    player.play(&asset("intro"))?;

    // 「ぜんぶ！」と言うまで無限に続く。
    // 何度でも好きな色を答えられるのがこの遊びの本体なので、
    // 回数の上限は設けない。
    let mut round: u32 = 0;
    loop {
        round += 1;

        // 1. 「どんないろがすき？」を再生（ト長調）
        //    まだ色が決まっていないので全色を出す。
        draw_all();
        player.play(&asset("question"))?;

        // 2. 質問の直後だけ聞く。
        //    原曲の m5 3-4拍目に「（あか！）」という合いの手が
        //    書かれており、そこがそのまま応答の枠になっている。
        //
        //    常時聞いていると自分が流した音を拾って誤爆するため、
        //    エコーキャンセルではなくウィンドウ制御で回避する。
        let heard = ears.hear(LISTEN_MAX)?;
        let answer = heard.as_deref().and_then(match_answer);

        match answer {
            // 3a. 「ぜんぶ！」→ 転調してぜんぶの節、そのままエンディング。
            Some(Answer::All) => {
                draw_all();
                player.play(&asset("all"))?;
                break;
            }
            // 3b. 色 → その節を再生してループ継続
            Some(Answer::Color(c)) => {
                draw_one(c);
                player.play(&asset(c.stem()))?;
            }
            // 3c. 何も言わなかった、または聞き取れなかった → ランダムな色。
            //     黙ってはいけない。ここで All を返してはならない。
            //     事故で終わってしまう。
            None => {
                let c = pick_random();
                draw_one(c);
                player.play(&asset(c.stem()))?;
            }
        }

        // 4. 3周に1回、区切りを挟む。同じ質問と節の往復だけだと単調になる。
        //    挟むものはブリッジと間奏を交互に入れ替える。
        //    同じ区切りが毎回続くとそれ自体が単調になるため。
        //
        //      3周目  → bridge    いろ いろ いろんな いろがある
        //      6周目  → interlude 間奏
        //      9周目  → bridge
        //      12周目 → interlude ...
        //
        //    どちらもト長調のまま終わるので、そのまま次の質問に戻れる。
        //
        //    「ぜんぶ！」で break した場合はここを通らない。
        //    エンディングの後に区切りが流れては困る。
        let insert = round % INSERT_EVERY == 0;
        let interlude_next = insert && (round / INSERT_EVERY) % 2 == 0;

        // 5. 節の最終小節（「ン」＋休符）。全12色で完全に同一なので
        //    色の素材からは外してある。
        //
        //    間奏へ向かうときだけ助走つきの tail-lead を使う。
        //    間奏を launch する B5→C6→D6 はアウフタクトで、
        //    この小節に属する。間奏側の頭に置くと拍の位置が変わってしまう。
        player.play(&asset(if interlude_next { "tail-lead" } else { "tail" }))?;

        if insert {
            // 区切りの間はまた全色に戻す。
            draw_all();
            player.play(&asset(if interlude_next { "interlude" } else { "bridge" }))?;
        }
    }

    Ok(())
}

/// 全色を並べて描く。質問・区切り・「ぜんぶ」のとき。
fn draw_all() {
    let rgbs: Vec<Rgb> = Color::ALL.iter().map(|c| c.rgb()).collect();
    print!("{}", display::all(&rgbs));
}

/// 1色を大きく描く。その色のフレーズを歌っている間。
fn draw_one(c: Color) {
    print!("{}", display::one(c.rgb()));
}

/// 判定できなかったときのフォールバック。
fn pick_random() -> Color {
    use rand::seq::SliceRandom;
    *Color::ALL.choose(&mut rand::thread_rng()).unwrap()
}

/// 「ぜんぶ」の読み。ここにマッチしたらゲーム終了。
const ALL_READINGS: &[&str] = &["ぜんぶ", "全部", "ぜーんぶ", "ぜんぶー"];

/// 判定に使う候補をすべて列挙する。
fn candidates() -> Vec<(Answer, &'static str)> {
    let mut v = Vec::new();
    for c in Color::ALL {
        for &r in c.readings() {
            v.push((Answer::Color(c), r));
        }
    }
    for &r in ALL_READINGS {
        v.push((Answer::All, r));
    }
    v
}

/// カタカナをひらがなに寄せ、空白と記号を落とす。
///
/// 認識結果と読みの両方を同じ関数に通すので、読みの側にカタカナ表記を
/// 並べる必要がない。長音符「ー」はカタカナ領域の外なのでそのまま残る。
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

/// 許容する編集距離。短い読みほど厳しくする。
/// 「あか」と「あお」は1文字違いなので、緩めると取り違える。
fn allowed(reading_len: usize) -> usize {
    if reading_len <= 4 {
        1
    } else {
        2
    }
}

/// 認識結果の文字列から応答を判定する。
///
/// 1. 完全一致
/// 2. 部分一致（長い読みから順に）
/// 3. 編集距離が最小のもの
/// 4. どれも該当しなければ `None`。呼び出し側でランダムな色に倒す
fn match_answer(raw: &str) -> Option<Answer> {
    let text = normalize(raw);
    if text.is_empty() {
        return None;
    }
    let mut cands: Vec<(Answer, String)> = candidates()
        .into_iter()
        .map(|(a, r)| (a, normalize(r)))
        .collect();

    // 1. 完全一致
    if let Some((a, _)) = cands.iter().find(|(_, r)| *r == text) {
        return Some(*a);
    }

    // 2. 部分一致。短い色名が長い色名に含まれる組（「きみどり」⊃「みどり」）が
    //    あるので、長い読みから試さないと取りこぼす。
    cands.sort_by_key(|(_, r)| std::cmp::Reverse(r.chars().count()));
    if let Some((a, _)) = cands.iter().find(|(_, r)| text.contains(r.as_str())) {
        return Some(*a);
    }

    // 3. 編集距離。
    //
    //    「ぜんぶ」はここでは判定しない。誤検出するとゲームが終わってしまう
    //    ため、あいまい一致まで許すのは危険が大きい。取りこぼしても次の周回で
    //    また聞けるので、そちらの害は小さい。
    let chars: Vec<char> = text.chars().collect();
    let mut scores: Vec<(Answer, usize)> = Vec::new();
    for (a, r) in cands.iter().filter(|(a, _)| *a != Answer::All) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn c(x: Color) -> Option<Answer> {
        Some(Answer::Color(x))
    }

    #[test]
    fn exact_match() {
        assert_eq!(match_answer("あか"), c(Color::Red));
        assert_eq!(match_answer("みずいろ"), c(Color::LightBlue));
        assert_eq!(match_answer("ぜんぶ"), Some(Answer::All));
    }

    #[test]
    fn katakana_is_normalized_to_hiragana() {
        assert_eq!(match_answer("アカ"), c(Color::Red));
        assert_eq!(match_answer("ピンク"), c(Color::Pink));
        assert_eq!(match_answer("オレンジ"), c(Color::Orange));
    }

    #[test]
    fn kanji_readings() {
        assert_eq!(match_answer("黄色"), c(Color::Yellow));
        assert_eq!(match_answer("黄緑"), c(Color::YellowGreen));
    }

    #[test]
    fn punctuation_and_space_are_stripped() {
        assert_eq!(match_answer(" あか！ "), c(Color::Red));
        assert_eq!(match_answer("あお。"), c(Color::Blue));
    }

    #[test]
    fn substring_prefers_the_longest_reading() {
        // 「きみどり」は「みどり」を含む。短いほうから見ると取りこぼす。
        assert_eq!(match_answer("きみどりがすき"), c(Color::YellowGreen));
        assert_eq!(match_answer("みどりだよ"), c(Color::Green));
    }

    #[test]
    fn fuzzy_match_absorbs_mispronunciation() {
        assert_eq!(match_answer("むらさぎ"), c(Color::Purple));
        assert_eq!(match_answer("みずいる"), c(Color::LightBlue));
    }

    #[test]
    fn ambiguous_one_char_difference_is_rejected() {
        // 「あか」からも「あお」からも距離1。割れたまま採用すると取り違える。
        assert_eq!(match_answer("あき"), None);
    }

    #[test]
    fn all_is_not_matched_fuzzily() {
        // 誤検出するとゲームが終わってしまうので、あいまい一致では拾わない。
        assert_eq!(match_answer("ぜんぷ"), None);
    }

    #[test]
    fn empty_input() {
        assert_eq!(match_answer(""), None);
        assert_eq!(match_answer("   "), None);
    }

    #[test]
    fn unrelated_speech_is_rejected() {
        assert_eq!(match_answer("おかあさん"), None);
    }

    #[test]
    fn levenshtein_basics() {
        let a: Vec<char> = "あか".chars().collect();
        let b: Vec<char> = "あお".chars().collect();
        assert_eq!(levenshtein(&a, &a), 0);
        assert_eq!(levenshtein(&a, &b), 1);
    }

    #[test]
    fn every_asset_stem_is_unique() {
        let mut stems: Vec<&str> = Color::ALL.iter().map(|c| c.stem()).collect();
        stems.sort_unstable();
        let n = stems.len();
        stems.dedup();
        assert_eq!(stems.len(), n);
    }
}
