//! 鳴らす素材。
//!
//! 文字列で指すと綴り間違いがビルドを通ってしまうので型にする。
//! `every()` に並べてあるものが、そのまま埋め込みビルドの対象になる。

use strum::{EnumCount, VariantArray};

use crate::color::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    /// イントロ。最初に一度だけ。
    Intro,
    /// どんないろがすき？
    Question,
    /// 節の最終小節。「ン」＋休符。
    Tail,
    /// 同上＋間奏への助走。間奏の直前だけ使う。
    TailLead,
    /// いろ いろ いろんな いろがある
    Bridge,
    /// 間奏
    Interlude,
    /// 転調 → ぜんぶの節 → エンディング
    Finale,
    /// 各色の節
    Color(Color),
}

impl Cue {
    /// 色に依らない素材。ここに並べたものが `every()` の前半になる。
    ///
    /// 色と違って読みや RGB のような付随情報が無く、名前も規則的でない
    /// （`Finale` は `all`、`TailLead` は `tail-lead`）ので、手で並べる。
    const COMMON: [Cue; 7] = [
        Cue::Intro,
        Cue::Question,
        Cue::Tail,
        Cue::TailLead,
        Cue::Bridge,
        Cue::Interlude,
        Cue::Finale,
    ];

    /// 素材の総数。
    ///
    /// `strum` の `EnumCount` は変種を数えるだけなので、`Cue` に付けても
    /// `Color(Color)` を1つと数えて 8 になる。欲しいのは鳴らせる素材の数
    /// なので、色のぶんを展開したこちらを使う。
    pub const COUNT: usize = Self::COMMON.len() + Color::COUNT;

    /// 音源のファイル名。`assets/<stem>.wav`。
    pub const fn stem(&self) -> &'static str {
        match self {
            Cue::Intro => "intro",
            Cue::Question => "question",
            Cue::Tail => "tail",
            Cue::TailLead => "tail-lead",
            Cue::Bridge => "bridge",
            Cue::Interlude => "interlude",
            Cue::Finale => "all",
            Cue::Color(c) => c.stem(),
        }
    }

    /// 素材の一覧。埋め込みビルドと素材チェックが参照する。
    pub const fn every() -> [Cue; Self::COUNT] {
        let mut out = [Cue::Intro; Self::COUNT];
        let mut i = 0;
        while i < Self::COMMON.len() {
            out[i] = Self::COMMON[i];
            i += 1;
        }
        let mut j = 0;
        while j < Color::COUNT {
            out[Self::COMMON.len() + j] = Cue::Color(Color::VARIANTS[j]);
            j += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_are_unique() {
        let mut stems: Vec<&str> = Cue::every().iter().map(|c| c.stem()).collect();
        let n = stems.len();
        stems.sort_unstable();
        stems.dedup();
        assert_eq!(stems.len(), n, "素材名が重複している");
    }

    /// const で受けられること自体が主張なので、const で受ける。
    #[test]
    fn every_covers_all_colors() {
        const EVERY: [Cue; Cue::COUNT] = Cue::every();

        assert_eq!(EVERY.len(), 7 + Color::COUNT);
        // 後半が色ぶん。1つも取りこぼしていない。
        for (i, &c) in Color::VARIANTS.iter().enumerate() {
            assert_eq!(EVERY[Cue::COMMON.len() + i], Cue::Color(c));
        }
    }
}
