//! 素材をどう鳴らすか。
//!
//! 実装は `audio::Speakers`（rodio）だけで、選択肢があるわけではない。
//! それでもトレイトにしてあるのは、**これが具象だと進行を一切テストできない**
//! ため。`Game` が受け取る4つのうち他の3つはトレイトなので、ここが実の
//! デバイスを開く限り、`Game` を組み立てられない。
//!
//! 長さ0を返す偽物を置けば、鳴らした `Cue` の並びをそのまま検査できて、
//! しかも待ち時間が消える。

use anyhow::Result;

use crate::cue::Cue;

/// 素材の長さ。
pub struct Timing {
    /// 末尾の無音まで含めた全長。
    pub total: std::time::Duration,
    /// 音が鳴り止むまで。
    pub audible: std::time::Duration,
}

/// 素材を鳴らすもの。
///
/// `Send` は要求しない。rodio の出力ストリームが `!Send` で、ワーカー
/// スレッドの中で開くことになっているため。
pub trait Player {
    /// 鳴らし終わるまで待つ。
    fn play(&self, cue: Cue) -> Result<()>;

    /// **音が鳴り止んだ時点で返す。** 末尾の無音は裏で流したままにする。
    fn play_until_quiet(&self, cue: Cue) -> Result<()>;

    /// **鳴らし始めて長さだけ返す。待たない。**
    fn begin(&self, cue: Cue) -> Result<Timing>;
}
