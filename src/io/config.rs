//! 設定の読み込み。ファイルと環境変数に触るのでこちら側。
//!
//! 値の出どころは3段。後のものが前を上書きする。
//!
//! 1. コード上の既定値（`Default`）
//! 2. `config.toml`（`--config <path>` か、なければカレントの `config.toml`）
//! 3. 環境変数

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use figment::providers::{Env, Format, Serialized, Toml};
use figment::Figment;

use crate::app::config::Config;

/// 環境変数の接頭辞。
const PREFIX: &str = "DONNA_IRO_";
/// 節とキーの区切り。キー名に `_` が入るので2本にする。
const NEST: &str = "__";

/// 既定値 → `config.toml` → 環境変数 の順に重ねる。
pub fn load(explicit: Option<&Path>) -> Result<Config> {
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
