//! `Control` の実装。

#[cfg(feature = "window")]
mod cec;
#[cfg(feature = "window")]
mod channel;
mod never;
mod stdin;

#[cfg(feature = "window")]
pub use cec::spawn as watch_remote;
#[cfg(feature = "window")]
pub use channel::Channel;
pub use never::Never;
pub use stdin::Stdin;
