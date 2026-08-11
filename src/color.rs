//! 認識対象の色。
//!
//! 語彙をここに閉じることで、汎用 ASR に頼らずとも判定できるようにする。

use strum::{EnumCount, VariantArray};

/// 光の三原色。色の言葉なのでここに置く。
///
/// 画面の側（`screen.rs`）に置いていたことがあり、そのせいで色が画面を
/// 読む向きになっていた。色は画面を知らなくても成り立つ。
pub type Rgb = (u8, u8, u8);

/// 歌がクレヨンの歌なので、標準的なクレヨン12色セット
/// （しろ・きいろ・きみどり・みどり・みずいろ・あお・むらさき・
/// ももいろ・あか・だいだい・ちゃいろ・くろ）に揃えてある。
/// 並び順は画面に出す順（4列×3行）でもある。`VARIANTS` は宣言順に返るので、
/// 並べ替えたいときはここを触ること。
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumCount, VariantArray)]
pub enum Color {
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
    /// 全色を宣言順に並べた配列。
    ///
    /// 一覧そのものは `VARIANTS` が持っている。ただしあちらはスライスなので、
    /// 並べ替えて持ち回る（フィナーレのシャッフル）には丈の決まった配列が要る。
    /// これはその入れ物への移し替えであって、一覧を二重に持つわけではない。
    ///
    /// `COUNT` と `VARIANTS` は同じ derive から出るので長さは必ず一致し、
    /// 添字が外れることはない。
    ///
    /// `std::array::from_fn` は const ではないので手で回している。const に
    /// しておくと定数として畳めて、`const` の初期値にも書ける。
    pub const fn all() -> [Color; Self::COUNT] {
        let mut out = [Color::VARIANTS[0]; Self::COUNT];
        let mut i = 1;
        while i < Self::COUNT {
            out[i] = Color::VARIANTS[i];
            i += 1;
        }
        out
    }

    /// 認識結果にマッチさせる読み。ひらがなの正式形をひとつだけ持つ。
    ///
    /// 表記ゆれや聞き間違いを並べていた時期もあったが、
    /// `initial_prompt` でこの語彙そのものを whisper に教えるように
    /// したので不要になった。残りのゆれは編集距離で吸収する。
    pub const fn reading(&self) -> &'static str {
        match self {
            Color::Red => "あか",
            Color::Blue => "あお",
            Color::Yellow => "きいろ",
            Color::Green => "みどり",
            Color::YellowGreen => "きみどり",
            Color::White => "しろ",
            Color::Black => "くろ",
            Color::Pink => "ぴんく",
            Color::Orange => "おれんじ",
            Color::Purple => "むらさき",
            Color::Brown => "ちゃいろ",
            Color::LightBlue => "みずいろ",
        }
    }

    /// 漢字表記。`initial_prompt` は誘導であって強制ではないので、
    /// whisper は漢字を返すことがある。ひらがなに機械変換できないため、
    /// 判定用に一形だけ持っておく。カタカナ表記の色は正規化で
    /// ひらがなに寄るので不要。
    pub const fn kanji(&self) -> Option<&'static str> {
        match self {
            Color::Red => Some("赤"),
            Color::Blue => Some("青"),
            Color::Yellow => Some("黄色"),
            Color::Green => Some("緑"),
            Color::YellowGreen => Some("黄緑"),
            Color::White => Some("白"),
            Color::Black => Some("黒"),
            Color::Purple => Some("紫"),
            Color::Brown => Some("茶色"),
            Color::LightBlue => Some("水色"),
            Color::Pink | Color::Orange => None,
        }
    }

    /// ターミナルに描く●の色。クレヨン12色の実際の色味に寄せてある。
    pub const fn rgb(&self) -> Rgb {
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

    /// ターミナルに出す名前。動作確認用なので読めれば良い。
    pub const fn name(&self) -> &'static str {
        match self {
            Color::Red => "赤",
            Color::Blue => "青",
            Color::Yellow => "黄",
            Color::Green => "緑",
            Color::YellowGreen => "黄緑",
            Color::White => "白",
            Color::Black => "黒",
            Color::Pink => "ピンク",
            Color::Orange => "オレンジ",
            Color::Purple => "紫",
            Color::Brown => "茶",
            Color::LightBlue => "水色",
        }
    }

    pub const fn stem(&self) -> &'static str {
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

    pub fn random() -> Color {
        use rand::seq::SliceRandom;
        *Color::VARIANTS.choose(&mut rand::thread_rng()).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// const で受けられること自体が主張なので、const で受ける。
    /// **この関数はコンパイルが通った時点で半分終わっている。**
    #[test]
    fn is_usable_in_const_context() {
        const ALL: [Color; Color::COUNT] = Color::all();
        const RED: &str = Color::Red.reading();

        assert_eq!(ALL.as_slice(), Color::VARIANTS, "一覧が VARIANTS とずれた");
        assert_eq!(RED, "あか");
    }
}
