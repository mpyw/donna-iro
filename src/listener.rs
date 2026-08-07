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

/// 応答を1回聞き取る。何も言わなければ `None`。
pub trait Listener {
    fn hear(&mut self, max: Duration) -> Result<Option<String>>;
}

/// 標準入力から色名を打つ。音源と進行の確認用。
pub struct Keyboard;

impl Listener for Keyboard {
    fn hear(&mut self, _max: Duration) -> Result<Option<String>> {
        print!("  いろは？ > ");
        std::io::stdout().flush()?;
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        let line = line.trim().to_string();
        Ok(if line.is_empty() { None } else { Some(line) })
    }
}

#[cfg(feature = "whisper")]
pub use mic::Mic;

#[cfg(feature = "whisper")]
mod mic {
    use std::time::{Duration, Instant};

    use anyhow::{Context, Result};
    use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

    use super::Listener;
    use crate::audio::{Ears, WHISPER_SR};
    use crate::color::Color;

    /// モデルも状態も起動時に一度だけ作る。
    ///
    /// `create_state()` は毎回 100MB 超の計算バッファを確保し直すので、
    /// 呼ぶたびに目に見えて遅くなるうえ初期化ログも出る。
    pub struct Mic {
        state: whisper_rs::WhisperState,
        ears: Ears,
        /// whisper に「こういう言葉が来る」と教える文。
        prompt: String,
    }

    /// 認識対象の語をひらがなで並べた文を作る。
    ///
    /// これを与えないと whisper が漢字に変換してしまう。実際に
    /// 「むらさき」が「村先」になった。同音の漢字は無限にあるので、
    /// 読みを足していく方式では追いつかない。出力そのものを寄せる。
    fn vocabulary() -> String {
        let mut words: Vec<&str> = Color::ALL.iter().map(|c| c.reading()).collect();
        words.push(crate::matcher::ALL_READING);
        words.join("、") + "。"
    }

    impl Mic {
        pub fn new(ears: Ears) -> Result<Self> {
            // whisper.cpp が stderr に吐く初期化ログを log クレートに流す。
            // ロガーを入れていないので、そのまま捨てられる。
            whisper_rs::install_logging_hooks();

            // tiny で足りることを実機で確認済み。語彙が12色に閉じていて
            // initial_prompt で誘導し、さらに編集距離で吸収するので、
            // 大きいモデルを積む理由がない。base に上げたければ
            // DONNA_IRO_MODEL で差し替える。
            let path = std::env::var("DONNA_IRO_MODEL")
                .unwrap_or_else(|_| "models/ggml-tiny.bin".to_string());
            let ctx = WhisperContext::new_with_params(&path, WhisperContextParameters::default())
                .with_context(|| format!("モデルを読めない: {path}"))?;
            let state = ctx.create_state()?;
            let prompt = vocabulary();
            eprintln!("  語彙: {prompt}");
            Ok(Self {
                state,
                ears,
                prompt,
            })
        }

        fn transcribe(&mut self, pcm: &[f32]) -> Option<String> {
            let started = Instant::now();
            let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

            // 語彙が色名に閉じているので、言語を固定して迷わせない。
            params.set_language(Some("ja"));
            // 出てくる語をあらかじめ教えて、漢字変換や言い換えを抑える。
            // no_context とは独立に効く（prompt_tokens 経由）。
            params.set_initial_prompt(&self.prompt);
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
            // 失敗したら編集距離マッチに任せるほうが速い。
            params.set_temperature(0.0);
            params.set_temperature_inc(0.0);
            params.set_suppress_blank(true);
            params.set_suppress_nst(true);

            self.state.full(params, pcm).ok()?;

            // 0.16 では full_n_segments() は i32 を直接返し、
            // テキストは get_segment() で取り出す。
            let mut text = String::new();
            for i in 0..self.state.full_n_segments() {
                if let Some(seg) = self.state.get_segment(i) {
                    if let Ok(s) = seg.to_str_lossy() {
                        text.push_str(&s);
                    }
                }
            }
            eprintln!(
                "  認識 {:.2}秒（音声 {:.1}秒 / audio_ctx {}）→ {:?}",
                started.elapsed().as_secs_f32(),
                pcm.len() as f32 / WHISPER_SR as f32,
                audio_ctx(pcm.len()),
                text.trim()
            );
            Some(text)
        }
    }

    impl Listener for Mic {
        fn hear(&mut self, max: Duration) -> Result<Option<String>> {
            match self.ears.listen(max)? {
                Some(pcm) => Ok(self.transcribe(&pcm)),
                None => Ok(None),
            }
        }
    }

    fn threads() -> i32 {
        std::thread::available_parallelism()
            .map(|n| n.get() as i32)
            .unwrap_or(4)
    }

    /// 30秒ぶんのエンコーダ文脈（1500）を、実際の音の長さに合わせて縮める。
    /// 小さくしすぎると精度が落ちるので下限を設けている。
    fn audio_ctx(samples: usize) -> i32 {
        let secs = samples as f32 / WHISPER_SR as f32;
        let proportional = ((secs + 1.0) / 30.0 * 1500.0).ceil() as i32;
        proportional.clamp(512, 1500)
    }
}
