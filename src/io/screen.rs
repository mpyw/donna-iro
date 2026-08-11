//! `Screen` の実装。

mod terminal;
#[cfg(feature = "window")]
pub mod window;

pub use terminal::Terminal;
#[cfg(feature = "window")]
pub use window::Remote;
