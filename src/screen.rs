//! 色をどこに出すか。
//!
//! 本命はウィンドウ（`window.rs`）。テレビに映して2歳児に見せるものなので、
//! ターミナルは動作確認用に色名を出すだけに留める。

use crate::color::Color;

pub type Rgb = (u8, u8, u8);

/// 今なにを映すか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame {
    /// 全色を並べる。質問・区切り・「ぜんぶ」のとき。
    Palette,
    /// 1色だけ大きく。その色のフレーズを歌っている間。
    Single(Color),
}

pub trait Screen: Send {
    fn show(&mut self, frame: Frame);
}

/// 枠の色。白と黒が背景に溶けないよう、明るい色は暗く、
/// 暗い色は明るくずらした縁を描く。
pub fn border(c: Rgb) -> Rgb {
    let lum = 0.299 * c.0 as f32 + 0.587 * c.1 as f32 + 0.114 * c.2 as f32;
    let f = |v: u8| {
        if lum < 100.0 {
            (v as f32 + 90.0).min(255.0) as u8
        } else {
            (v as f32 * 0.55) as u8
        }
    };
    (f(c.0), f(c.1), f(c.2))
}

/// 色名を出すだけ。動作確認用。
///
/// ANSI で四角を描く版もあったが、フォントやエミュレータに左右されて
/// 環境によって崩れる。見せる相手は2歳児でターミナルは見ないので、
/// ここは進行が追えれば足りる。
pub struct Terminal;

impl Screen for Terminal {
    fn show(&mut self, frame: Frame) {
        match frame {
            Frame::Palette => println!("── ぜんぶ ──"),
            Frame::Single(c) => println!("● {}", c.name()),
        }
    }
}
