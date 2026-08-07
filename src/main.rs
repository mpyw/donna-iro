//! 「どんな色がすき？」と呼びかけ、子どもが叫んだ色名を聞き取って
//! その色の続きを再生する。
//!
//! 設計方針は README を参照。要点は「無反応にしない」こと。
//! 色が判定できなくても必ず何かを再生する。

use anyhow::Result;

/// 認識対象の色。語彙をこれだけに閉じることで、
/// 汎用 ASR に頼らずとも判定できるようにする。
///
/// 2歳児が実際に口にする色を優先して選んである。
/// はいいろ・きんいろ等はまず言わないので入れていない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Color {
    Red,
    Blue,
    Yellow,
    Green,
    White,
    Black,
    Pink,
    Orange,
    Purple,
    Brown,
    LightBlue,
}

impl Color {
    const ALL: [Color; 11] = [
        Color::Red,
        Color::Blue,
        Color::Yellow,
        Color::Green,
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
    /// マッチは**長い読みから順に**試すこと。「みずいろ」を「いろ」で
    /// 拾ってしまうような取りこぼしを避けるため。
    fn readings(&self) -> &'static [&'static str] {
        match self {
            Color::Red => &["あか", "アカ", "赤"],
            Color::Blue => &["あお", "アオ", "青"],
            Color::Yellow => &["きいろ", "キイロ", "黄色", "きーろ", "きいく"],
            Color::Green => &["みどり", "ミドリ", "緑", "みろり"],
            Color::White => &["しろ", "シロ", "白"],
            Color::Black => &["くろ", "クロ", "黒"],
            Color::Pink => &["ぴんく", "ピンク", "ぴんこ"],
            Color::Orange => &["おれんじ", "オレンジ", "おえんじ"],
            Color::Purple => &["むらさき", "ムラサキ", "紫", "むあさき"],
            Color::Brown => &["ちゃいろ", "チャイロ", "茶色"],
            Color::LightBlue => &["みずいろ", "ミズイロ", "水色"],
        }
    }

    /// 再生する音源。サブモジュール `assets/` (private) 内のパス。
    ///
    /// **各ファイルにはフレーズを最後まで収録すること。**
    /// 「あかいいろがすき」だけで切れていると歌として成立しない。
    /// 何を吹き込むかは assets リポジトリの README を参照。
    fn asset(&self) -> &'static str {
        match self {
            Color::Red => "assets/red.wav",
            Color::Blue => "assets/blue.wav",
            Color::Yellow => "assets/yellow.wav",
            Color::Green => "assets/green.wav",
            Color::White => "assets/white.wav",
            Color::Black => "assets/black.wav",
            Color::Pink => "assets/pink.wav",
            Color::Orange => "assets/orange.wav",
            Color::Purple => "assets/purple.wav",
            Color::Brown => "assets/brown.wav",
            Color::LightBlue => "assets/lightblue.wav",
        }
    }
}

fn main() -> Result<()> {
    loop {
        // 1. 「どんないろがすき？」を再生
        play("assets/question.wav")?;

        // 2. 質問の直後だけ録音する。
        //    常時聞いていると自分が流した音を拾って誤爆するため、
        //    エコーキャンセルではなくウィンドウ制御で回避する。
        let audio = record(std::time::Duration::from_secs(3))?;

        // 3. 色を判定する。判定できなければランダムに選ぶ。
        //    ここで None を返して黙ると子どもが興味を失うので、
        //    必ず何かを返すこと。
        let color = recognize(&audio).unwrap_or_else(pick_random);

        // 4. その色の続きを再生。フレーズは最後まで流し切る。
        play(color.asset())?;
    }
}

/// 音声から色を判定する。確信が持てなければ None。
///
/// TODO: まず whisper-rs で文字起こし → readings() にあいまい一致、で試す。
///       精度が出なければ、その子の声で学習した専用分類器に差し替える。
fn recognize(_audio: &[f32]) -> Option<Color> {
    todo!("whisper-rs による認識")
}

/// 判定できなかったときのフォールバック。
fn pick_random() -> Color {
    use rand::seq::SliceRandom;
    *Color::ALL.choose(&mut rand::thread_rng()).unwrap()
}

/// TODO: cpal で録音する。返り値は 16kHz モノラルの f32 PCM（whisper の入力形式）。
fn record(_duration: std::time::Duration) -> Result<Vec<f32>> {
    todo!("cpal による録音")
}

/// TODO: rodio で再生する。再生完了までブロックする。
fn play(_path: &str) -> Result<()> {
    todo!("rodio による再生")
}
