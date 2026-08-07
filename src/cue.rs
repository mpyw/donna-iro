//! 鳴らす素材。
//!
//! 文字列で指すと綴り間違いがビルドを通ってしまうので型にする。
//! `ALL` に並べてあるものが、そのまま埋め込みビルドの対象になる。

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
    pub fn stem(&self) -> &'static str {
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
    pub fn every() -> Vec<Cue> {
        let mut v = vec![
            Cue::Intro,
            Cue::Question,
            Cue::Tail,
            Cue::TailLead,
            Cue::Bridge,
            Cue::Interlude,
            Cue::Finale,
        ];
        v.extend(Color::ALL.iter().map(|&c| Cue::Color(c)));
        v
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

    #[test]
    fn every_covers_all_colors() {
        assert_eq!(Cue::every().len(), 7 + Color::ALL.len());
    }
}
