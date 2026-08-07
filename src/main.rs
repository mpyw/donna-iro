//! 「どんな色がすき？」と呼びかけ、子どもが叫んだ色名を聞き取って
//! その色の続きを再生する。
//!
//! 設計方針は README を参照。要点は「無反応にしない」こと。
//! 色が判定できなくても必ず何かを再生する。

use anyhow::Result;

/// 認識対象の色。語彙をこれだけに閉じることで、
/// 汎用 ASR に頼らずとも判定できるようにする。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Color {
    Red,
    Blue,
    Yellow,
    Green,
    White,
    Brown,
}

impl Color {
    const ALL: [Color; 6] = [
        Color::Red,
        Color::Blue,
        Color::Yellow,
        Color::Green,
        Color::White,
        Color::Brown,
    ];

    /// 認識結果の文字列にマッチさせる読み。表記ゆれを吸収する。
    fn readings(&self) -> &'static [&'static str] {
        match self {
            Color::Red => &["あか", "アカ", "赤"],
            Color::Blue => &["あお", "アオ", "青"],
            Color::Yellow => &["きいろ", "キイロ", "黄色", "きーろ"],
            Color::Green => &["みどり", "ミドリ", "緑"],
            Color::White => &["しろ", "シロ", "白"],
            Color::Brown => &["ちゃいろ", "チャイロ", "茶色"],
        }
    }

    fn asset(&self) -> &'static str {
        match self {
            Color::Red => "assets/red.wav",
            Color::Blue => "assets/blue.wav",
            Color::Yellow => "assets/yellow.wav",
            Color::Green => "assets/green.wav",
            Color::White => "assets/white.wav",
            Color::Brown => "assets/brown.wav",
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

        // 4. その色の続きを再生
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
