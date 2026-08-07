//! 子どもの応答をどう受け取るか。
//!
//! マイク（whisper）とキーボードの2通り。キーボードは再生や進行を
//! 確かめるためのもので、whisper.cpp のビルドを待たずに動かせる。
//!
//!     cargo run --no-default-features   キーボード
//!     cargo run                         マイク

use std::io::Write;
use std::time::Duration;

use anyhow::Result;

use crate::audio;

pub enum Listener {
    Mic(Mic),
    Keyboard,
}

impl Listener {
    /// 応答を聞き取って文字列にする。何も言わなければ `None`。
    pub fn hear(&mut self, max: Duration) -> Result<Option<String>> {
        match self {
            Listener::Mic(m) => match audio::listen(max)? {
                Some(pcm) => Ok(m.transcribe(&pcm)),
                None => Ok(None),
            },
            Listener::Keyboard => {
                print!("  いろは？ > ");
                std::io::stdout().flush()?;
                let mut line = String::new();
                std::io::stdin().read_line(&mut line)?;
                let line = line.trim().to_string();
                Ok(if line.is_empty() { None } else { Some(line) })
            }
        }
    }
}

#[cfg(not(feature = "whisper"))]
pub struct Mic;

#[cfg(not(feature = "whisper"))]
impl Mic {
    pub fn new() -> Result<Self> {
        anyhow::bail!("whisper フィーチャーを有効にしてビルドしてください")
    }
    pub fn transcribe(&mut self, _pcm: &[f32]) -> Option<String> {
        None
    }
}

#[cfg(feature = "whisper")]
pub use whisper_impl::Mic;

#[cfg(feature = "whisper")]
mod whisper_impl {
    use anyhow::{Context, Result};
    use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

    /// モデルは起動時に一度だけ読む。毎回読み直すと数秒かかる。
    pub struct Mic {
        ctx: WhisperContext,
    }

    impl Mic {
        pub fn new() -> Result<Self> {
            let path = std::env::var("DONNA_IRO_MODEL")
                .unwrap_or_else(|_| "models/ggml-base.bin".to_string());
            let ctx = WhisperContext::new_with_params(&path, WhisperContextParameters::default())
                .with_context(|| format!("モデルを読めない: {path}"))?;
            Ok(Self { ctx })
        }

        pub fn transcribe(&mut self, pcm: &[f32]) -> Option<String> {
            let mut state = self.ctx.create_state().ok()?;
            let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
            // 語彙が色名に閉じているので、言語を固定して迷わせない。
            params.set_language(Some("ja"));
            params.set_translate(false);
            params.set_print_special(false);
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_timestamps(false);
            state.full(params, pcm).ok()?;

            let n = state.full_n_segments().ok()?;
            let mut text = String::new();
            for i in 0..n {
                text.push_str(&state.full_get_segment_text(i).ok()?);
            }
            Some(text)
        }
    }
}
