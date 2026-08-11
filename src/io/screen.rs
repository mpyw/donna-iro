//! `Screen` の実装。

mod terminal;
#[cfg(feature = "window")]
mod window;

pub use terminal::Terminal;
// ウィンドウはメインスレッドで回す必要があるので、描き手（`Remote`）と
// 回し手（`run`）が別れている。**中の割り方は外に見せない。**
#[cfg(feature = "window")]
pub use window::{run, Remote};
