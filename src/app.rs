//! 遊びそのもの。**装置にもファイルにも触らない。**
//!
//! ここに置くものは、鳴らす・映す・聞くの実体を知らない。何を鳴らすか、
//! 何を映すか、聞こえたものをどう解釈するか、までを決めてトレイトに渡す。
//! 実体は `io` にあり、`main` が繋ぐ。
//!
//! **`app` から `io` を読んではいけない。** 逆は構わない。この向きが
//! 守られている限り、遊びの側は装置なしで組み立てられる（`game` の
//! テストがそれをやっている）。
//!
//! 唯一の例外は時間。`game` は `Instant::now` と `sleep` を使う。装置や
//! ファイルは触らないので、ここでは純粋として扱う。

mod color;
mod control;
mod cue;
mod game;
mod listener;
mod matcher;
mod player;
mod screen;

// 遊びの口。`main` と `io` はここだけ見れば繋げられるので、モジュールを
// 辿らずに書けるようにしておく。**中の割り方を外に見せない。**
pub use color::{Answer, Color, Rgb};
pub use control::Control;
pub use cue::Cue;
pub use game::Game;
pub use listener::{Heard, Listener};
pub use player::{Player, Timing};
pub use screen::{Frame, Screen};
