//! 色をどこに出すか。
//!
//! 何を映すか（`Frame`）と、映す先の口（`Screen`）だけ。実際に描くのは
//! `io::screen` で、本命はウィンドウ、ターミナルは動作確認用。

use strum::EnumCount;

use crate::app::color::Color;

/// 今なにを映すか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame {
    /// 全色を並べる。質問・区切りのとき。
    ///
    /// 並び順を持つのは、フィナーレで位置ごとに色を入れ替えるため。
    /// 通常は宣言順（`Color::VARIANTS`）。
    Palette([Color; Color::COUNT]),
    /// 1色だけ大きく。その色のフレーズを歌っている間。
    Single(Color),
    /// 「もう1回」を待っている。
    ///
    /// 待っていることも、押せば続くことも、画面に出さないと誰にも
    /// 分からない。ターミナルは `control::Stdin` が自前で促すので、
    /// これが要るのは実質ウィンドウのほう。
    Again,
}

impl Frame {
    /// 既定の並びの全色。
    pub const fn palette() -> Frame {
        Frame::Palette(Color::ALL)
    }
}

pub trait Screen: Send {
    fn show(&mut self, frame: Frame);
}
