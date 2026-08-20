//! マイクと whisper による `Listener`。本番はこちら。

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::app::{Answer, Heard, Listener};
use crate::config::Config;
use crate::io::audio::{Ears, WHISPER_SR};

/// モデルも状態も起動時に一度だけ作る。
///
/// `create_state()` は毎回 100MB 超の計算バッファを確保し直すので、
/// 呼ぶたびに目に見えて遅くなるうえ初期化ログも出る。
pub struct Mic {
    state: whisper_rs::WhisperState,
    ears: Ears,
    audio_ctx: i32,
}

/// `initial_prompt` に渡す語彙。読みを「、」で繋いで「。」で閉じる。
///
/// これを与えないと whisper が漢字に変換してしまう。実際に「むらさき」が
/// 「村先」になった。同音の漢字は無限にあるので、読みを足していく方式では
/// 追いつかない。出力そのものを寄せる。
///
/// 語彙は `Answer` から出るので、色を足せばここも伸びる。
///
/// **const で組んでいた時期もあったが、起動時に一度作るだけのものに
/// バイト操作50行と、それを検証するテストは重すぎた。**
static VOCABULARY: LazyLock<String> = LazyLock::new(|| {
    let readings: Vec<&str> = Answer::every().iter().map(|a| a.reading()).collect();
    readings.join("、") + "。"
});

impl Mic {
    pub fn new(ears: Ears, cfg: &Config) -> Result<Self> {
        // whisper.cpp が stderr に吐く初期化ログを log クレートに流す。
        // ロガーを入れていないので、そのまま捨てられる。
        whisper_rs::install_logging_hooks();

        let ctx = load_model(cfg)?;
        let state = ctx.create_state()?;
        eprintln!("  語彙: {}", *VOCABULARY);
        Ok(Self {
            state,
            ears,
            // 範囲は config の検証で保証されている。ここで黙って直さない。
            audio_ctx: cfg.recognize.audio_ctx,
        })
    }

    fn transcribe(&mut self, pcm: &[f32]) -> Option<String> {
        let started = Instant::now();
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

        // 語彙が色名に閉じているので、言語を固定して迷わせない。
        params.set_language(Some("ja"));
        // 出てくる語をあらかじめ教えて、漢字変換や言い換えを抑える。
        // no_context とは独立に効く（prompt_tokens 経由）。
        params.set_initial_prompt(&VOCABULARY);
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
        let audio_ctx = audio_ctx_for(pcm.len(), self.audio_ctx);
        params.set_audio_ctx(audio_ctx);

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

        // 認識そのものが失敗したら、黙って「言わなかった」に倒れる。
        // ランダムな色に倒す設計は正しいが、恒常的に壊れていても気づけない
        // のは別問題なので、ここだけは出す。
        if let Err(e) = self.state.full(params, pcm) {
            eprintln!("  認識に失敗: {e}");
            return None;
        }

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
            audio_ctx,
            text.trim()
        );
        Some(text)
    }
}

impl Listener for Mic {
    fn hear(&mut self, max: Duration) -> Result<Heard> {
        // マイクは絶えない。聞こえなければ「言わなかった」。
        let Some(pcm) = self.ears.listen(max)? else {
            return Ok(Heard::Nothing);
        };
        Ok(match self.transcribe(&pcm) {
            Some(text) => Heard::Said(text),
            None => Heard::Nothing,
        })
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

/// 30秒 = 1500 なので、**1秒あたり 50。**
const CTX_PER_SEC: i32 = 50;
/// 端の畳み込みで欠けるぶんと、測り損ねたぶんの余裕（1秒ぶん）。
const CTX_MARGIN: i32 = CTX_PER_SEC;
/// これより下は削っても速くならないので刻まない。config の下限と揃えてある。
const CTX_MIN: i32 = 128;
/// whisper の上限（= 30秒、切り詰めなし）。
const CTX_MAX: i32 = 1500;

/// 音声の長さから `audio_ctx` を決める。
///
/// **パディングには情報が無い。** whisper は必ず30秒に詰めてからエンコード
/// するが、実際に鳴ったぶんを覆っていれば残りを見る意味は無く、覆う範囲なら
/// 精度は落ちない。落ちるのは実音声より短く切ったときだけ。
///
/// ラズパイ実測（音声 2.0〜2.3秒、大人の声）:
///
/// ```text
/// audio_ctx 1500 → 2.79秒 / 2.72秒   判定 あか / ぜんぶ
/// audio_ctx  384 → 0.66秒 / 0.78秒   判定 あか / ぜんぶ（同じ）
/// ```
///
/// `pinned` が 0 でなければそれをそのまま使う。**外したときに固定へ戻せる
/// 逃げ道を残す**ため。子どもの声で精度が出なかったとき、モデルを大きく
/// するよりここを戻すほうが効いた、という別の実測がある。
fn audio_ctx_for(samples: usize, pinned: i32) -> i32 {
    if pinned != 0 {
        return pinned;
    }
    let secs = samples as f32 / WHISPER_SR as f32;
    let need = (secs * CTX_PER_SEC as f32).ceil() as i32;
    need.saturating_add(CTX_MARGIN).clamp(CTX_MIN, CTX_MAX)
}

fn threads() -> i32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **固定されていたらそれを使う。** 導出が常に勝つと、精度が出なかった
    /// ときに戻す先が無くなる。
    #[test]
    fn a_pinned_value_wins() {
        assert_eq!(audio_ctx_for(WHISPER_SR as usize * 2, 1500), 1500);
        assert_eq!(audio_ctx_for(WHISPER_SR as usize * 2, 384), 384);
    }

    /// 1秒あたり 50 に余裕を足したぶん。
    #[test]
    fn derives_from_the_length_plus_a_margin() {
        let secs = |s: f32| (WHISPER_SR as f32 * s) as usize;
        assert_eq!(audio_ctx_for(secs(2.0), 0), 150);
        assert_eq!(audio_ctx_for(secs(2.2), 0), 160);
    }

    /// **実音声を必ず覆うこと。** ここが割れると答えの尻が切れる。
    /// 覆えているかは「長さ×50 以上か」で見る。
    #[test]
    fn always_covers_the_audio_it_is_given() {
        for tenths in 1..=300 {
            let secs = tenths as f32 / 10.0;
            let samples = (WHISPER_SR as f32 * secs) as usize;
            let ctx = audio_ctx_for(samples, 0);
            let need = (secs * CTX_PER_SEC as f32).ceil() as i32;
            assert!(
                ctx >= need,
                "{secs}秒 は {need} 要るのに {ctx} しか見ていない"
            );
        }
    }

    /// 短すぎるものは刻まず、長すぎるものは whisper の上限で止まる。
    #[test]
    fn stays_inside_the_range_whisper_accepts() {
        assert_eq!(audio_ctx_for(0, 0), CTX_MIN);
        assert_eq!(audio_ctx_for(WHISPER_SR as usize / 10, 0), CTX_MIN);
        // 30秒を超えて渡されても、whisper が受ける上限で止まる。
        assert_eq!(audio_ctx_for(WHISPER_SR as usize * 60, 0), CTX_MAX);
    }
}
