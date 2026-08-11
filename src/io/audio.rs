//! 装置と素材。`app` のトレイトを実装するわけではなく、`io::player` と
//! `io::listener::mic` が下敷きに使う層。
//!
//! cpal も rodio もクロスプラットフォームで、macOS では CoreAudio、
//! Raspberry Pi では ALSA を裏で使う。ソースは共通のままでよい。

mod assets;
mod clip;
/// 録音は whisper のときだけ要る。手打ちのビルドではまるごと不要。
#[cfg(feature = "whisper")]
mod ears;

pub use assets::configure;
pub use clip::Clip;
#[cfg(feature = "whisper")]
pub use ears::Ears;

/// whisper が受け取るサンプリングレート。
#[cfg(feature = "whisper")]
pub const WHISPER_SR: u32 = 16_000;

/// これを下回れば無音とみなす振幅。再生側の末尾検出と録音側の
/// 打ち切り判定で同じ基準を使う。
const SILENCE: f32 = 0.02;
