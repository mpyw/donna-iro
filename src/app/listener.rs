//! 子どもの応答をどう受け取るか。
//!
//! 実装は `io::listener`。マイク（whisper）とキーボードの2通り。キーボードは再生や進行を
//! 確かめるためのもので、whisper.cpp のビルドを待たずに動かせる。
//!
//!     cargo run --no-default-features   キーボード
//!     cargo run                         マイク

use std::time::Duration;

use anyhow::Result;

/// 応答を1回聞き取る。何も言わなければ `None`。
pub trait Listener {
    fn hear(&mut self, max: Duration) -> Result<Option<String>>;
}
