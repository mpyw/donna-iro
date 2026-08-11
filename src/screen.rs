//! 色をどこに出すか。
//!
//! 本命はウィンドウ（`window.rs`）。テレビに映して2歳児に見せるものなので、
//! ターミナルは動作確認用に色名を出すだけに留める。

use strum::{EnumCount, VariantArray};

use crate::color::{Color, Rgb};

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
        Frame::Palette(Color::all())
    }
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
            Frame::Palette(order) if order.as_slice() == Color::VARIANTS => println!("── ぜんぶ ──"),
            Frame::Palette(order) => {
                let names: Vec<&str> = order.iter().map(|c| c.name()).collect();
                println!("── {} ──", names.join(" "));
            }
            Frame::Single(c) => println!("● {}", c.name()),
            // `control::Stdin` がこのあとプロンプトを出すので、二重に言わない。
            Frame::Again => {}
        }
    }
}
