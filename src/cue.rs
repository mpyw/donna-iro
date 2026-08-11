//! 鳴らす素材。
//!
//! 文字列で指すと綴り間違いがビルドを通ってしまうので型にする。
//! `every()` に並べてあるものが、そのまま埋め込みビルドの対象になる。

use strum::{EnumCount, VariantArray};

use crate::color::Color;
use const_for::const_for;

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumCount)]
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
    ///
    /// **丈は変種の数から出す。** ペイロードを持つのは `Color(Color)` だけ
    /// なので、そのぶんを引いた数がここに並ぶべき数になる。素材を足して
    /// ここに書き忘れると、丈が合わずビルドが止まる。
    ///
    /// `Cue::COUNT` と書けないのは、固有の const（下の、鳴らせる素材の数）が
    /// 優先されて循環するため。数えたいのは変種のほうなので名指しする。
    const COMMON: [Cue; <Cue as EnumCount>::COUNT - 1] = [
        Cue::Intro,
        Cue::Question,
        Cue::Tail,
        Cue::TailLead,
        Cue::Bridge,
        Cue::Interlude,
        Cue::Finale,
    ];

    /// 鳴らせる素材の総数。
    ///
    /// `EnumCount` が数えるのは変種なので、`Color(Color)` を1つと数えて
    /// 8 にしかならない。欲しいのは色を展開した数なのでこちらを持つ。
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
        const_for!(i in 0..Self::COMMON.len() => {
            out[i] = Self::COMMON[i];
        });
        const_for!(j in 0..Color::COUNT => {
            out[Self::COMMON.len() + j] = Cue::Color(Color::VARIANTS[j]);
        });
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

    /// `every()` は const fn の中で添字を手で進めている。丈は型が見て
    /// いるので、ここでは**中身が「COMMON のあとに全色」になっているか**を
    /// 見る。const で受けられること自体も主張なので、const で受ける。
    #[test]
    fn every_is_common_then_all_colors() {
        const EVERY: [Cue; Cue::COUNT] = Cue::every();

        for (i, &c) in Cue::COMMON.iter().enumerate() {
            assert_eq!(EVERY[i], c, "前半が COMMON と揃っていない");
        }
        for (i, &c) in Color::VARIANTS.iter().enumerate() {
            assert_eq!(
                EVERY[Cue::COMMON.len() + i],
                Cue::Color(c),
                "後半に色を取りこぼしている"
            );
        }
    }
}
