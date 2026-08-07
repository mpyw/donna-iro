//! 「どんな色がすき？」と呼びかけ、子どもが叫んだ色名を聞き取って
//! その色の続きを再生する。
//!
//! 設計方針は README を参照。要点は「無反応にしない」こと。
//! 色が判定できなくても必ず何かを再生する。

use anyhow::Result;

/// 認識対象の色。語彙をこれだけに閉じることで、
/// 汎用 ASR に頼らずとも判定できるようにする。
///
/// 歌がクレヨンの歌なので、標準的なクレヨン12色セット
/// （しろ・きいろ・きみどり・みどり・みずいろ・あお・むらさき・
/// ももいろ・あか・だいだい・ちゃいろ・くろ）を基準に選んである。
/// そこに こん・はいいろ と、子どもが好きな きんいろ・ぎんいろ を足した16色。
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
    Navy,
    Gray,
    Gold,
    Silver,
}

impl Color {
    const ALL: [Color; 16] = [
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
        Color::Navy,
        Color::Gray,
        Color::Gold,
        Color::Silver,
    ];

    /// 認識結果の文字列にマッチさせる読み。表記ゆれと幼児の発音ゆれを吸収する。
    ///
    /// マッチは**必ず長い読みから順に**試すこと。短い色名が長い色名に
    /// 含まれる組み合わせがあるため、素朴に前から探すと誤判定する。
    ///
    /// - 「きみどり」⊃「みどり」
    /// - 「きんいろ」「ぎんいろ」「みずいろ」「ちゃいろ」「はいいろ」⊃「いろ」
    ///
    /// 「ももいろ」はピンク、「だいだい」はオレンジの読みとして扱う。
    /// クレヨンの表記と子どもが言う語が違うため、両方拾えるようにしてある。
    fn readings(&self) -> &'static [&'static str] {
        match self {
            Color::Red => &["あか", "アカ", "赤"],
            Color::Blue => &["あお", "アオ", "青"],
            Color::Yellow => &["きいろ", "キイロ", "黄色", "きーろ", "きいく"],
            Color::Green => &["みどり", "ミドリ", "緑", "みろり"],
            Color::YellowGreen => &["きみどり", "キミドリ", "黄緑", "きみろり"],
            Color::White => &["しろ", "シロ", "白"],
            Color::Black => &["くろ", "クロ", "黒"],
            Color::Pink => &["ぴんく", "ピンク", "ぴんこ", "ももいろ", "桃色"],
            Color::Orange => &["おれんじ", "オレンジ", "おえんじ", "だいだい", "橙"],
            Color::Purple => &["むらさき", "ムラサキ", "紫", "むあさき"],
            Color::Brown => &["ちゃいろ", "チャイロ", "茶色"],
            Color::LightBlue => &["みずいろ", "ミズイロ", "水色"],
            Color::Navy => &["こん", "コン", "紺", "こんいろ", "紺色"],
            Color::Gray => &["はいいろ", "ハイイロ", "灰色", "ぐれー", "グレー"],
            Color::Gold => &["きんいろ", "キンイロ", "金色", "きん", "ゴールド"],
            Color::Silver => &["ぎんいろ", "ギンイロ", "銀色", "ぎん", "シルバー"],
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
            Color::YellowGreen => "assets/yellowgreen.wav",
            Color::White => "assets/white.wav",
            Color::Black => "assets/black.wav",
            Color::Pink => "assets/pink.wav",
            Color::Orange => "assets/orange.wav",
            Color::Purple => "assets/purple.wav",
            Color::Brown => "assets/brown.wav",
            Color::LightBlue => "assets/lightblue.wav",
            Color::Navy => "assets/navy.wav",
            Color::Gray => "assets/gray.wav",
            Color::Gold => "assets/gold.wav",
            Color::Silver => "assets/silver.wav",
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
