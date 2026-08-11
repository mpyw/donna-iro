//! ターミナルに色名を出すだけの `Screen`。

use strum::VariantArray;

use crate::app::color::Color;

use crate::app::{Frame, Screen};

/// 色名を出すだけ。動作確認用。
///
/// ANSI で四角を描く版もあったが、フォントやエミュレータに左右されて
/// 環境によって崩れる。見せる相手は2歳児でターミナルは見ないので、
/// ここは進行が追えれば足りる。
pub struct Terminal;

impl Screen for Terminal {
    fn show(&mut self, frame: Frame) {
        match frame {
            Frame::Palette(order) if order.as_slice() == Color::VARIANTS => {
                println!("── ぜんぶ ──")
            }
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
