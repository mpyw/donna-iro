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

    fn threads() -> i32 {
        std::thread::available_parallelism()
            .map(|n| n.get() as i32)
            .unwrap_or(4)
    }

    /// 30秒ぶんのエンコーダ文脈（1500）を、実際の音の長さに合わせて縮める。
    /// 小さくしすぎると精度が落ちるので下限を設けている。
    fn audio_ctx(samples: usize) -> i32 {
        let secs = samples as f32 / crate::audio::WHISPER_SR as f32;
        let proportional = ((secs + 1.0) / 30.0 * 1500.0).ceil() as i32;
        proportional.clamp(512, 1500)
    }

    /// モデルも状態も起動時に一度だけ作る。
    ///
    /// `create_state()` は毎回 100MB 超の計算バッファを確保し直すので、
    /// 呼ぶたびに目に見えて遅くなるうえ初期化ログも出る。
    pub struct Mic {
        state: whisper_rs::WhisperState,
    }

    impl Mic {
        pub fn new() -> Result<Self> {
            // whisper.cpp が stderr に吐く初期化ログを log クレートに流す。
            // ロガーを入れていないので、そのまま捨てられる。
            whisper_rs::install_logging_hooks();
            let path = std::env::var("DONNA_IRO_MODEL")
                .unwrap_or_else(|_| "models/ggml-base.bin".to_string());
            let ctx = WhisperContext::new_with_params(&path, WhisperContextParameters::default())
                .with_context(|| format!("モデルを読めない: {path}"))?;
            let state = ctx.create_state()?;
            Ok(Self { state })
        }

        pub fn transcribe(&mut self, pcm: &[f32]) -> Option<String> {
            let started = std::time::Instant::now();
            let state = &mut self.state;
            let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

            // 語彙が色名に閉じているので、言語を固定して迷わせない。
            params.set_language(Some("ja"));
            params.set_translate(false);
            params.set_print_special(false);
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_timestamps(false);

            // --- ここから下は速度のための設定 ---

            params.set_n_threads(threads());

            // whisper は入力を必ず30秒に詰めてからエンコードする。
            // audio_ctx を実際の長さに見合う値まで下げると、
            // エンコーダの計算量がそのぶん減る。ここが一番効く。
            params.set_audio_ctx(audio_ctx(pcm.len()));

            // 一言しか来ないので、区切らず・文脈も持たない。
            params.set_single_segment(true);
            params.set_no_context(true);
            params.set_token_timestamps(false);

            // 色名は数トークンで終わる。上限を切って無駄な復号を止める。
            params.set_max_tokens(16);

            // 温度を上げて何度もやり直すフォールバックを止める。
            // 失敗したらこちらの編集距離マッチに任せるほうが速い。
            params.set_temperature(0.0);
            params.set_temperature_inc(0.0);
            params.set_suppress_blank(true);
            params.set_suppress_nst(true);

            state.full(params, pcm).ok()?;

            // 0.16 では full_n_segments() は i32 を直接返し、
            // テキストは get_segment() で取り出す。
            let mut text = String::new();
            for i in 0..state.full_n_segments() {
                if let Some(seg) = state.get_segment(i) {
                    if let Ok(s) = seg.to_str_lossy() {
                        text.push_str(&s);
                    }
                }
            }
            eprintln!(
                "  認識 {:.2}秒（音声 {:.1}秒 / audio_ctx {}）→ {:?}",
                started.elapsed().as_secs_f32(),
                pcm.len() as f32 / crate::audio::WHISPER_SR as f32,
                audio_ctx(pcm.len()),
                text.trim()
            );
            Some(text)
        }
    }
}
