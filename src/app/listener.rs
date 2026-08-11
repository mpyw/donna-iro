//! 子どもの応答をどう受け取るか。
//!
//! 実装は `io::listener`。マイク（whisper）とキーボードの2通り。キーボードは再生や進行を
//! 確かめるためのもので、whisper.cpp のビルドを待たずに動かせる。
//!
//!     cargo run --no-default-features   キーボード
//!     cargo run                         マイク

use std::time::Duration;

use anyhow::Result;

/// 聞き取りの結果。
///
/// **「言わなかった」と「入力が絶えた」を分ける。** どちらも `None` に
/// 畳んでいた頃は、手打ちで Ctrl-D を押すとランダムな色を延々と鳴らし
/// 続けた。前者はランダムな色に倒すべきで、後者は終わるべきもの。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    /// 何か言った。中身の解釈は `Matcher` に任せる。
    Said(String),
    /// 何も言わなかった。ランダムな色に倒す。
    Nothing,
    /// 入力そのものが絶えた。遊びを終える。
    Closed,
}

/// 応答を1回聞き取る。`max` は待つ上限で、声が途切れたら早く返してよい。
pub trait Listener {
    fn hear(&mut self, max: Duration) -> Result<Heard>;
}
