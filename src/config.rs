//! 設定。**`app` からも `io` からも読んでよい第三の層。**
//!
//! 遊びの側でも装置の側でも「どう振る舞うか」を決めるのに要るので、
//! どちらか片方に寄せると必ずもう一方から手が伸びる。型と読み込みを
//! 割って置いていた時期もあったが、継ぎ目が増えるだけだった。
//!
//! **`load` を呼ぶのは `main` だけ。** `app` が触るのは値と既定値で、
//! ファイルや環境変数を読む側ではない。ここを守っている限り、遊びの側は
//! 装置なしで組み立てられる。
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

use anyhow::{Context, Result};
use figment::providers::{Env, Format, Serialized, Toml};
use figment::Figment;
use serde::{Deserialize, Serialize};

use std::time::Duration;

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
    assets: String,
    /// whisper のモデル。埋め込みビルドでもこれが指定されていれば
    /// ファイルから読む。
    model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Listen {
    /// 使う入力デバイス。名前の一部で選ぶ。空なら OS の既定。
    ///
    /// **読むときは `device()` を通すこと。** 空文字が「指定なし」の合図
    /// なので、生で見るとその意味を各所で書き直すことになる。
    device: String,
    /// 応答を待つ上限（秒）。
    max_seconds: f32,
    /// 声が途切れてから打ち切るまでの猶予（ミリ秒）。
    hangover_ms: u64,
    /// 窓を開けた直後、スピーカーの残響を無視する時間（ミリ秒）。
    guard_ms: u64,
    /// これだけ続けて初めて「聞こえた」とみなす（ミリ秒）。
    min_speech_ms: u64,
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
    flash_ms: u64,
}

impl Default for Listen {
    fn default() -> Self {
        Self {
            device: String::new(),
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
    /// 名前で選ぶなら、その一部。空なら OS の既定に任せる。
    pub fn device(&self) -> Option<&str> {
        (!self.device.is_empty()).then_some(self.device.as_str())
    }

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
    /// **起動時に値の筋を通す。** 素材と同じで、遊んでいる最中に気づくのが
    /// 一番まずい。
    ///
    /// 検査しないと、正しい TOML を書いただけで実行時に落ちる。
    /// `max_seconds` が負なら `Duration::from_secs_f32` が、
    /// `speech_floor > speech_ceil` なら `clamp` が、聞き取りのたびに
    /// パニックする。`insert_every = 0` は割り算、`flash_ms = 0` は
    /// フィナーレのビジーループになる。
    ///
    /// **「正かつ有限」では足りない。** `max_seconds = 1e30` はそれを通るが
    /// `Duration::from_secs_f32` は変換できずに落ちる。上限まで見る。
    fn validate(&self) -> Result<()> {
        // 範囲で見る。閉区間。
        let between = |name: &str, v: f32, lo: f32, hi: f32| -> Result<()> {
            if v.is_finite() && (lo..=hi).contains(&v) {
                Ok(())
            } else {
                anyhow::bail!("{name} は {lo} 〜 {hi} であること（いまは {v}）")
            }
        };

        // 上限は「これを超えたら設定を間違えている」の線。
        //
        // 1分待つ玩具は使い物にならないし、`Duration` に載らない値や、
        // 録音バッファの上限計算（`rate * (max + 2)`）が桁溢れする値も
        // ここで落ちる。
        const MAX_WAIT: f32 = 60.0;
        // 振幅は ±1 に正規化してある。1を超えるしきい値は永久に発火しない。
        const MAX_LEVEL: f32 = 1.0;

        between(
            "listen.max_seconds",
            self.listen.max_seconds,
            0.01,
            MAX_WAIT,
        )?;
        between("listen.speech_ratio", self.listen.speech_ratio, 0.01, 100.0)?;
        between(
            "listen.speech_floor",
            self.listen.speech_floor,
            0.0,
            MAX_LEVEL,
        )?;
        between(
            "listen.speech_ceil",
            self.listen.speech_ceil,
            0.0,
            MAX_LEVEL,
        )?;
        between("listen.threshold", self.listen.threshold, 0.0, MAX_LEVEL)?;

        // **窓に収まらない下拵えは、永久に無言と同じ。** 残響を無視する時間と
        // 発話が続くべき時間の合計が上限を超えると、`listen` は毎回
        // 「聞こえなかった」で返る。落ちも警告も出ないぶん、こちらのほうが
        // 見つけにくい。
        // 検証器自身が落ちては元も子もない。デバッグビルドの `+` は溢れる。
        let head = self
            .listen
            .guard_ms
            .saturating_add(self.listen.min_speech_ms);
        let window_ms = (self.listen.max_seconds * 1000.0) as u64;
        if head >= window_ms {
            anyhow::bail!(
                "listen.guard_ms + min_speech_ms（{head}ms）が max_seconds（{window_ms}ms）に\
                 収まっていない。このままだと何を言っても無言になる"
            );
        }

        if self.listen.speech_floor > self.listen.speech_ceil {
            anyhow::bail!(
                "listen.speech_floor は speech_ceil 以下であること（いまは {} > {}）",
                self.listen.speech_floor,
                self.listen.speech_ceil
            );
        }
        if self.game.insert_every == 0 {
            anyhow::bail!("game.insert_every は1以上であること（0だと割り算で落ちる）");
        }
        if self.game.flash_ms == 0 {
            anyhow::bail!("game.flash_ms は1以上であること（0だとフィナーレが空回りする）");
        }
        if self.recognize.head_segments == 0 {
            anyhow::bail!("recognize.head_segments は1以上であること");
        }
        if !(128..=1500).contains(&self.recognize.audio_ctx) {
            anyhow::bail!(
                "recognize.audio_ctx は 128〜1500 であること（いまは {}）",
                self.recognize.audio_ctx
            );
        }
        Ok(())
    }
}

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
        // **file ではなく file_exact。** file は無いファイルを黙って空の
        // プロバイダにするので、--config のタイプミスが既定値での起動に
        // なる。指定したのに効いていないのが一番たちが悪い。
        figment = figment.merge(Toml::file_exact(p));
    }
    let cfg: Config = figment
        .merge(Env::prefixed(PREFIX).split(NEST))
        .extract()
        .context("設定を読めない")?;

    cfg.validate()?;

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

#[cfg(test)]
mod tests {
    use super::*;

    /// 検証は**起動時に**効くこと。素材と同じで、遊んでいる最中に気づくのが
    /// 一番まずい。ここが緩むと、正しい TOML を書いただけで実行時に落ちる。
    #[test]
    fn rejects_values_that_would_panic_later() {
        let bad = |f: fn(&mut Config)| {
            let mut cfg = Config::default();
            f(&mut cfg);
            cfg.validate().unwrap_err().to_string()
        };

        // Duration::from_secs_f32 がパニックする
        assert!(bad(|c| c.listen.max_seconds = -1.0).contains("max_seconds"));
        assert!(bad(|c| c.listen.max_seconds = f32::NAN).contains("max_seconds"));
        // **上限側でもパニックする。** 「正かつ有限」だけ見ていた頃はここが
        // 素通りして、Duration に変換できずに落ちていた。
        assert!(bad(|c| c.listen.max_seconds = 1.0e30).contains("max_seconds"));
        assert!(bad(|c| c.listen.max_seconds = f32::INFINITY).contains("max_seconds"));
        // 振幅は ±1 まで。それを超えるしきい値は永久に発火しない
        assert!(bad(|c| c.listen.threshold = 2.0).contains("threshold"));
        // 下拵えが窓に収まらない = 何を言っても無言
        assert!(bad(|c| c.listen.guard_ms = 9_000).contains("収まっていない"));
        assert!(bad(|c| c.listen.min_speech_ms = 9_000).contains("収まっていない"));
        // clamp がパニックする。しかも聞き取りのたび
        assert!(bad(|c| c.listen.speech_floor = 0.9).contains("speech_ceil"));
        // 割り算
        assert!(bad(|c| c.game.insert_every = 0).contains("insert_every"));
        // フィナーレが空回りする
        assert!(bad(|c| c.game.flash_ms = 0).contains("flash_ms"));
        // whisper 側が黙って直していた範囲
        assert!(bad(|c| c.recognize.audio_ctx = 64).contains("audio_ctx"));
    }

    #[test]
    fn accepts_the_shipped_defaults() {
        Config::default()
            .validate()
            .expect("既定値が自分の検証に落ちている");
    }

    /// 空文字は「指定なし」の合図。生のフィールドを読むとその意味を
    /// 各所で書き直すことになるので、アクセサに寄せてある。
    #[test]
    fn empty_device_means_the_os_default() {
        let mut cfg = Config::default();
        assert_eq!(cfg.listen.device(), None);
        cfg.listen.device = "Nuroum".to_string();
        assert_eq!(cfg.listen.device(), Some("Nuroum"));
    }
}
