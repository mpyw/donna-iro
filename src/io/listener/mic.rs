//! マイクと whisper による `Listener`。本番はこちら。

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::app::color::Answer;
use crate::app::config::Config;
use crate::app::listener::Listener;
use crate::io::audio::{Ears, WHISPER_SR};
use const_for::const_for;

/// モデルも状態も起動時に一度だけ作る。
///
/// `create_state()` は毎回 100MB 超の計算バッファを確保し直すので、
/// 呼ぶたびに目に見えて遅くなるうえ初期化ログも出る。
pub struct Mic {
    state: whisper_rs::WhisperState,
    ears: Ears,
    audio_ctx: i32,
}

/// 語彙を繋ぐ区切りと、末尾。
const JOIN: &str = "、";
const END: &str = "。";

/// `VOCABULARY` のバイト長。読み＋区切り＋末尾。
const fn vocabulary_len() -> usize {
    let every = Answer::every();
    let mut n = 0;
    const_for!(i in 0..Answer::COUNT => {
        n += every[i].reading().len();
        if i + 1 < Answer::COUNT {
            n += JOIN.len();
        }
    });
    n + END.len()
}

/// `src` を `dst` の `at` から書き、次に書ける位置を返す。
///
/// const では `copy_from_slice` が使えないので手で回す。3箇所で同じ
/// ことをするので括り出してある。
const fn put(dst: &mut [u8], at: usize, src: &[u8]) -> usize {
    const_for!(i in 0..src.len() => {
        dst[at + i] = src[i];
    });
    at + src.len()
}

/// 実体。`from_utf8` に渡すために、先に置き場所を用意する。
static VOCABULARY_BYTES: [u8; vocabulary_len()] = {
    let every = Answer::every();
    let mut out = [0u8; vocabulary_len()];
    let mut n = 0;
    const_for!(i in 0..Answer::COUNT => {
        n = put(&mut out, n, every[i].reading().as_bytes());
        if i + 1 < Answer::COUNT {
            n = put(&mut out, n, JOIN.as_bytes());
        }
    });
    put(&mut out, n, END.as_bytes());
    out
};

/// 認識対象の語をひらがなで並べた文。
///
/// これを与えないと whisper が漢字に変換してしまう。実際に
/// 「むらさき」が「村先」になった。同音の漢字は無限にあるので、
/// 読みを足していく方式では追いつかない。出力そのものを寄せる。
///
/// 語彙は `Answer` から出るので、色を足せばここも伸びる。
pub const VOCABULARY: &str = match std::str::from_utf8(&VOCABULARY_BYTES) {
    Ok(s) => s,
    Err(_) => panic!("語彙は UTF-8 のはず"),
};

impl Mic {
    pub fn new(ears: Ears, cfg: &Config) -> Result<Self> {
        // whisper.cpp が stderr に吐く初期化ログを log クレートに流す。
        // ロガーを入れていないので、そのまま捨てられる。
        whisper_rs::install_logging_hooks();

        let ctx = load_model(cfg)?;
        let state = ctx.create_state()?;
        eprintln!("  語彙: {VOCABULARY}");
        Ok(Self {
            state,
            ears,
            audio_ctx: cfg.recognize.audio_ctx.clamp(128, 1500),
        })
    }

    fn transcribe(&mut self, pcm: &[f32]) -> Option<String> {
        let started = Instant::now();
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

        // 語彙が色名に閉じているので、言語を固定して迷わせない。
        params.set_language(Some("ja"));
        // 出てくる語をあらかじめ教えて、漢字変換や言い換えを抑える。
        // no_context とは独立に効く（prompt_tokens 経由）。
        params.set_initial_prompt(VOCABULARY);
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
        params.set_audio_ctx(self.audio_ctx);

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
            self.audio_ctx,
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

/// 既定は base。tiny + `audio_ctx` full でも動くが、子どもの声には
/// base のほうが余裕がある。macOS では Metal が既定で効くので、
/// 速度の心配も要らない。ラズパイで遅ければ tiny に落とすか、
/// `audio_ctx` を下げる。
///
/// `paths.model`（`DONNA_IRO_PATHS__MODEL`）を指定すればファイルから読む。
/// 埋め込みビルドでもこれが優先されるので、別のモデルを試したいときに使える。
fn load_model(cfg: &Config) -> Result<WhisperContext> {
    if let Some(path) = cfg.paths.model() {
        return WhisperContext::new_with_params(path, WhisperContextParameters::default())
            .with_context(|| format!("モデルを読めない: {path}"));
    }

    // whisper.cpp はバッファから読むだけで書き換えないので、
    // 読み取り専用の静的領域をそのまま渡してよい。
    #[cfg(feature = "embed-model")]
    {
        const MODEL: &[u8] =
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/models/ggml-base.bin"));
        return WhisperContext::new_from_buffer_with_params(
            MODEL,
            WhisperContextParameters::default(),
        )
        .context("埋め込みモデルを読めない");
    }

    #[cfg(not(feature = "embed-model"))]
    {
        let path = "models/ggml-base.bin";
        WhisperContext::new_with_params(path, WhisperContextParameters::default())
            .with_context(|| format!("モデルを読めない: {path}（tools/fetch-model.sh で取得）"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// const で組み立てているので、中身がずれても型は通ってしまう。
    /// 元の実装（実行時に join する形）と突き合わせる。
    #[test]
    fn vocabulary_lists_every_reading() {
        let words: Vec<&str> = Answer::every().iter().map(|a| a.reading()).collect();
        assert_eq!(VOCABULARY, words.join(JOIN) + END);
        // 色を足したら伸びる。取りこぼすと whisper が漢字に化ける。
        assert_eq!(VOCABULARY.matches(JOIN).count(), Answer::COUNT - 1);
    }
}

fn threads() -> i32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(4)
}
