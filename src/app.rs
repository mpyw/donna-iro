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

pub mod color;
pub mod config;
pub mod control;
pub mod cue;
pub mod game;
pub mod listener;
pub mod matcher;
pub mod player;
pub mod screen;
