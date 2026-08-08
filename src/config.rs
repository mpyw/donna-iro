//! 設定。
//!
//! 値の出どころは3段。後のものが前を上書きする。
//!
//! 1. コード上の既定値（`Default`）
//! 2. `config.toml`（`--config <path>` か、なければカレントの `config.toml`）
//! 3. 環境変数
//!
//! **環境変数名はキーの位置から機械的に決まる。**
//! 節とキーの区切りは `__`（2本）。`listen.max_seconds` なら
//! `DONNA_IRO_LISTEN__MAX_SECONDS`。キー名自体に `_` が入るので、
//! 節の区切りは別の記号にする必要がある。
//!
//! 同梱の `config.toml` に全項目と対応する環境変数名が書いてあるので、
//! どこをいじれるかはそのファイルを見れば分かる。

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use figment::providers::{Env, Format, Serialized, Toml};
use figment::Figment;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub paths: Paths,
    pub listen: Listen,
    pub recognize: Recognize,
    pub game: Game,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Paths {
    /// 音源のディレクトリ。空なら自動（`assets/` が揃っていればそれ、
    /// なければ `assets/reference/`）。
    pub assets: String,
    /// whisper のモデル。埋め込みビルドでもこれが指定されていれば
    /// ファイルから読む。
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Listen {
    /// 応答を待つ上限（秒）。
    pub max_seconds: f32,
    /// 声が途切れてから打ち切るまでの猶予（ミリ秒）。
    pub hangover_ms: u64,
    /// 窓を開けた直後、スピーカーの残響を無視する時間（ミリ秒）。
    pub guard_ms: u64,
    /// これだけ続けて初めて「聞こえた」とみなす（ミリ秒）。
    pub min_speech_ms: u64,
    /// 環境ノイズの何倍を発話とみなすか。
    pub speech_ratio: f32,
    /// しきい値の下限。
    pub speech_floor: f32,
    /// しきい値の上限。
    pub speech_ceil: f32,
    /// 0 より大きければしきい値を直接指定する（環境ノイズを見ない）。
    pub threshold: f32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Recognize {
    /// エンコーダの文脈長。下げると速く、精度は落ちる。1500 で切り詰めなし。
    pub audio_ctx: i32,
    /// 認識結果の先頭いくつの区間を信用するか。
    pub head_segments: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Game {
    /// 何周に1回、区切り（ブリッジまたは間奏）を挟むか。
    pub insert_every: u32,
    /// フィナーレで色を差し替える間隔（ミリ秒）。
    pub flash_ms: u64,
}

impl Default for Listen {
    fn default() -> Self {
        Self {
            max_seconds: 5.0,
            hangover_ms: 350,
            guard_ms: 200,
            min_speech_ms: 75,
            speech_ratio: 2.0,
            speech_floor: 0.005,
            speech_ceil: 0.015,
            threshold: 0.0,
        }
    }
}

impl Default for Recognize {
    fn default() -> Self {
        Self {
            audio_ctx: 1500,
            head_segments: 2,
        }
    }
}

impl Default for Game {
    fn default() -> Self {
        Self {
            insert_every: 3,
            flash_ms: 500,
        }
    }
}

impl Listen {
    pub fn max(&self) -> Duration {
        Duration::from_secs_f32(self.max_seconds)
    }
    pub fn hangover(&self) -> Duration {
        Duration::from_millis(self.hangover_ms)
    }
    pub fn guard(&self) -> Duration {
        Duration::from_millis(self.guard_ms)
    }
    pub fn min_speech(&self) -> Duration {
        Duration::from_millis(self.min_speech_ms)
    }
}

impl Game {
    pub fn flash(&self) -> Duration {
        Duration::from_millis(self.flash_ms)
    }
}

impl Paths {
    pub fn assets(&self) -> Option<PathBuf> {
        (!self.assets.is_empty()).then(|| PathBuf::from(&self.assets))
    }
    pub fn model(&self) -> Option<&str> {
        (!self.model.is_empty()).then_some(self.model.as_str())
    }
}

/// 環境変数の接頭辞。
const PREFIX: &str = "DONNA_IRO_";
/// 節とキーの区切り。キー名に `_` が入るので2本にする。
const NEST: &str = "__";

impl Config {
    /// 既定値 → `config.toml` → 環境変数 の順に重ねる。
    pub fn load(explicit: Option<&Path>) -> Result<Self> {
        let path = explicit.map(Path::to_path_buf).or_else(|| {
            let d = PathBuf::from("config.toml");
            d.exists().then_some(d)
        });

        let mut figment = Figment::from(Serialized::defaults(Config::default()));
        if let Some(p) = &path {
            figment = figment.merge(Toml::file(p));
        }
        let cfg: Config = figment
            .merge(Env::prefixed(PREFIX).split(NEST))
            .extract()
            .context("設定を読めない")?;

        if let Some(p) = &path {
            eprintln!("  設定 {}", p.display());
        }
        let overridden: Vec<String> = std::env::vars()
            .filter_map(|(k, _)| k.strip_prefix(PREFIX).map(str::to_lowercase))
            .map(|k| k.replace(NEST, "."))
            .collect();
        if !overridden.is_empty() {
            eprintln!("  環境変数で上書き: {}", overridden.join(", "));
        }
        Ok(cfg)
    }
}
