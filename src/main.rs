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
    /// マッチは**必ず長い読みから順に**試すこと。短い色名が長い色名に
    /// 含まれる組み合わせがあるため、素朴に前から探すと誤判定する。
    ///
    /// - 「きみどり」⊃「みどり」
    /// - 「みずいろ」「ちゃいろ」「きいろ」⊃「いろ」
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
        }
    }
}

/// 子どもの応答。
enum Answer {
    /// 色を答えた。ループを続ける。
    Color(Color),
    /// 「ぜんぶ！」と答えた。転調してエンディングへ向かう。
    All,
}

fn main() -> Result<()> {
    // イントロは最初に一度だけ。
    play("assets/intro.wav")?;

    // 「ぜんぶ！」と言うまで無限に続く。
    // 何度でも好きな色を答えられるのがこの遊びの本体なので、
    // 回数の上限は設けない。
    let mut round: u32 = 0;
    loop {
        round += 1;

        // 1. 「どんないろがすき？」を再生（ト長調）
        play("assets/question.wav")?;

        // 2. 質問の直後だけ録音する。
        //    原曲の m5 3-4拍目に「（あか！）」という合いの手が
        //    書かれており、そこがそのまま応答の枠になっている。
        //
        //    常時聞いていると自分が流した音を拾って誤爆するため、
        //    エコーキャンセルではなくウィンドウ制御で回避する。
        //
        //    2歳児は考えてから言うので、最大5秒待つ。
        //    ただし毎回5秒待ち切ると間延びして歌にならないので、
        //    声が途切れた時点で早く切り上げること（LISTEN_MAX は上限）。
        let audio = listen(LISTEN_MAX)?;

        match recognize(&audio) {
            // 3a. 「ぜんぶ！」→ 転調してぜんぶの節、そのままエンディング。
            Some(Answer::All) => {
                play("assets/all.wav")?;
                break;
            }
            // 3b. 色 → その節を再生してループ継続
            Some(Answer::Color(c)) => play(c.asset())?,
            // 3c. 何も言わなかった、または聞き取れなかった → ランダムな色。
            //     黙ってはいけない。ここで All を返してはならない。
            //     事故で終わってしまう。
            None => play(pick_random().asset())?,
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
        if round % INSERT_EVERY == 0 {
            let nth = round / INSERT_EVERY;
            if nth % 2 == 1 {
                play("assets/bridge.wav")?;
            } else {
                play("assets/interlude.wav")?;
            }
        }
    }

    Ok(())
}

/// 何周に1回、区切り（ブリッジまたは間奏）を挟むか。
const INSERT_EVERY: u32 = 3;

/// 音声から応答を判定する。確信が持てなければ None。
///
/// **`All` の判定は色より慎重に。** 誤検出するとゲームが終わってしまう。
/// 逆に取りこぼしても次の周回でまた聞けるので、迷ったら色として扱うか
/// None を返すほうが害が小さい。
///
/// TODO: まず whisper-rs で文字起こし → readings() にあいまい一致、で試す。
///       精度が出なければ、その子の声で学習した専用分類器に差し替える。
fn recognize(_audio: &[f32]) -> Option<Answer> {
    todo!("whisper-rs による認識")
}

/// 「ぜんぶ」の読み。ここにマッチしたらゲーム終了。
const ALL_READINGS: &[&str] = &["ぜんぶ", "ゼンブ", "全部", "ぜーんぶ", "ぜんぶー"];

/// 判定できなかったときのフォールバック。
fn pick_random() -> Color {
    use rand::seq::SliceRandom;
    *Color::ALL.choose(&mut rand::thread_rng()).unwrap()
}

/// 応答を待つ最大時間。2歳児は考えてから言うので短すぎると取りこぼす。
const LISTEN_MAX: std::time::Duration = std::time::Duration::from_secs(5);

/// 応答を待って録音する。返り値は 16kHz モノラルの f32 PCM（whisper の入力形式）。
///
/// `max` は**上限**であって固定長ではない。声が途切れたら早く返すこと。
/// 毎回5秒待ち切ると歌の流れが死ぬ。
/// 何も言わなければ `max` まで待って、無音のまま返す（呼び出し側で
/// ランダムな色にフォールバックする）。
///
/// TODO: cpal で録音。打ち切り判定は素朴な音量しきい値でまず十分。
///       誤爆するようなら VAD（webrtc-vad など）を検討する。
fn listen(_max: std::time::Duration) -> Result<Vec<f32>> {
    todo!("cpal による録音と無音打ち切り")
}

/// TODO: rodio で再生する。再生完了までブロックする。
fn play(_path: &str) -> Result<()> {
    todo!("rodio による再生")
}
