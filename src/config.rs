//! 設定。
//!
//! 値の出どころは3段。後のものが前を上書きする。
//!
//! 1. コード上の既定値（`Default`）
//! 2. `config.toml`（`--config <path>` か、なければカレントの `config.toml`）
//! 3. 環境変数
//!
//! **環境変数名はキーの位置から機械的に決まる。**
//! `listen.max_seconds` なら `DONNA_IRO_LISTEN_MAX_SECONDS`。
//! 同梱の `config.toml` に全項目と対応する環境変数名が書いてあるので、
//! どこをいじれるかはそのファイルを見れば分かる。

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub paths: Paths,
    pub listen: Listen,
    pub recognize: Recognize,
    pub game: Game,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Paths {
    /// 音源のディレクトリ。空なら自動（`assets/` が揃っていればそれ、
    /// なければ `assets/reference/`）。
    pub assets: String,
    /// whisper のモデル。埋め込みビルドでもこれが指定されていれば
    /// ファイルから読む。
    pub model: String,
}

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Recognize {
    /// エンコーダの文脈長。下げると速く、精度は落ちる。1500 で切り詰めなし。
    pub audio_ctx: i32,
    /// 認識結果の先頭いくつの区間を信用するか。
    pub head_segments: usize,
}

#[derive(Debug, Deserialize)]
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

impl Config {
    /// 読み込んで環境変数で上書きする。
    pub fn load(explicit: Option<&Path>) -> Result<Self> {
        let path = explicit.map(Path::to_path_buf).or_else(|| {
            let d = PathBuf::from("config.toml");
            d.exists().then_some(d)
        });

        let mut cfg = match &path {
            Some(p) => {
                let text = std::fs::read_to_string(p)
                    .with_context(|| format!("設定を読めない: {}", p.display()))?;
                toml::from_str(&text)
                    .with_context(|| format!("設定の書式が違う: {}", p.display()))?
            }
            None => Config::default(),
        };
        let overridden = cfg.apply_env()?;

        if let Some(p) = &path {
            eprintln!("  設定 {}", p.display());
        }
        if !overridden.is_empty() {
            eprintln!("  環境変数で上書き: {}", overridden.join(", "));
        }
        Ok(cfg)
    }

    /// 環境変数で上書きする。名前はキーの位置から機械的に決まる。
    /// 上書きしたキーを返す。
    fn apply_env(&mut self) -> Result<Vec<String>> {
        let mut hit = Vec::new();
        macro_rules! env {
            ($($section:ident . $key:ident),* $(,)?) => {
                $(
                    let name = concat!(
                        "DONNA_IRO_",
                        stringify!($section), "_", stringify!($key)
                    ).to_uppercase();
                    if let Ok(raw) = std::env::var(&name) {
                        self.$section.$key = parse(&raw, &name)?;
                        hit.push(format!("{}.{}", stringify!($section), stringify!($key)));
                    }
                )*
            };
        }
        env!(
            paths.assets,
            paths.model,
            listen.max_seconds,
            listen.hangover_ms,
            listen.guard_ms,
            listen.min_speech_ms,
            listen.speech_ratio,
            listen.speech_floor,
            listen.speech_ceil,
            listen.threshold,
            recognize.audio_ctx,
            recognize.head_segments,
            game.insert_every,
            game.flash_ms,
        );
        Ok(hit)
    }
}

fn parse<T: std::str::FromStr>(raw: &str, name: &str) -> Result<T> {
    raw.parse()
        .map_err(|_| anyhow::anyhow!("{name} の値が読めない: {raw:?}"))
}
