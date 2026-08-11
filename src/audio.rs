//! 録音と再生。
//!
//! cpal も rodio もクロスプラットフォームで、macOS では CoreAudio、
//! Raspberry Pi では ALSA を裏で使う。ソースは共通のままでよい。

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
#[cfg(feature = "embed-audio")]
use const_for::const_for;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rodio::Source;

use crate::config::{Config, Listen};
use crate::cue::Cue;
use crate::player::{Player, Timing};

/// whisper が受け取るサンプリングレート。
pub const WHISPER_SR: u32 = 16_000;

/// これを下回れば無音とみなす振幅。再生側の末尾検出と録音側の
/// 打ち切り判定で同じ基準を使う。
const SILENCE: f32 = 0.02;

// ---------------------------------------------------------------- 再生

/// 出力ストリームを持ち回す。再生のたびに開き直すと
/// デバイスの初期化で無視できない間が空く。
pub struct Speakers {
    _stream: rodio::OutputStream,
    handle: rodio::OutputStreamHandle,
}

impl Speakers {
    pub fn new() -> Result<Self> {
        let (_stream, handle) =
            rodio::OutputStream::try_default().context("出力デバイスを開けない")?;
        Ok(Self { _stream, handle })
    }
}

impl Player for Speakers {
    fn play(&self, cue: Cue) -> Result<()> {
        std::thread::sleep(self.begin(cue)?.total);
        Ok(())
    }

    /// **音が鳴り止んだ時点で返す。** 末尾の無音は裏で流したままにする。
    ///
    /// `question.wav` の末尾には原曲の合いの手枠が約1秒ぶん無音で入って
    /// いる。そこがまさに子どもが答える瞬間なので、鳴らし切ってから
    /// 聞き始めると完全に手遅れになる。
    ///
    /// 無音の長さは素材から測る。決め打ちにすると、つくよみちゃんの音源に
    /// 差し替えたときに合わなくなる。
    fn play_until_quiet(&self, cue: Cue) -> Result<()> {
        std::thread::sleep(self.begin(cue)?.audible);
        Ok(())
    }

    /// **鳴らし始めて長さだけ返す。待たない。**
    ///
    /// 鳴っている間に画面を動かしたいときに使う。
    fn begin(&self, cue: Cue) -> Result<Timing> {
        let clip = Clip::load(cue)?;
        let timing = Timing {
            total: clip.total(),
            audible: clip.audible(),
        };
        let sink = rodio::Sink::try_new(&self.handle)?;
        sink.append(clip.into_source());
        // 呼び出し側が待つので、こちらは裏で流し切らせる。
        sink.detach();
        Ok(timing)
    }
}

/// 復号済みの音。長さと末尾の無音を測るために一度メモリに載せる。
struct Clip {
    samples: Vec<f32>,
    channels: u16,
    rate: u32,
}

impl Clip {
    fn load(cue: Cue) -> Result<Self> {
        let decoder = rodio::Decoder::new(Media::open(cue)?)?;
        let channels = decoder.channels();
        let rate = decoder.sample_rate();
        let samples: Vec<f32> = decoder.convert_samples().collect();
        Ok(Self {
            samples,
            channels,
            rate,
        })
    }

    fn frames(&self, n: usize) -> Duration {
        Duration::from_secs_f64(n as f64 / self.rate as f64)
    }

    fn total(&self) -> Duration {
        self.frames(self.samples.len() / self.channels as usize)
    }

    /// 末尾の無音を除いた長さ。
    fn audible(&self) -> Duration {
        let last = self
            .samples
            .iter()
            .rposition(|s| s.abs() > SILENCE)
            .map(|i| i / self.channels as usize + 1)
            .unwrap_or(0);
        self.frames(last)
    }

    fn into_source(self) -> rodio::buffer::SamplesBuffer<f32> {
        rodio::buffer::SamplesBuffer::new(self.channels, self.rate, self.samples)
    }
}

// ---------------------------------------------------------------- 素材の在処

/// ファイルから読むか、バイナリに埋め込んだものを読むか。
/// `rodio::Decoder` に渡すため、どちらも同じ型にまとめる。
enum Media {
    File(BufReader<File>),
    #[cfg(feature = "embed-audio")]
    Memory(std::io::Cursor<&'static [u8]>),
}

impl Media {
    fn open(cue: Cue) -> Result<Self> {
        let stem = cue.stem();

        // 埋め込みがあり、かつディレクトリ指定で上書きされていなければ
        // バイナリの中から鳴らす。
        #[cfg(feature = "embed-audio")]
        if asset_dir_override().is_none() {
            let bytes =
                embedded(stem).with_context(|| format!("埋め込まれていない素材: {stem}"))?;
            return Ok(Media::Memory(std::io::Cursor::new(bytes)));
        }

        let path = asset_path(stem);
        let file = File::open(&path).with_context(|| format!("音源がない: {}", path.display()))?;
        Ok(Media::File(BufReader::new(file)))
    }
}

impl Read for Media {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Media::File(f) => f.read(buf),
            #[cfg(feature = "embed-audio")]
            Media::Memory(c) => c.read(buf),
        }
    }
}

impl Seek for Media {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        match self {
            Media::File(f) => f.seek(pos),
            #[cfg(feature = "embed-audio")]
            Media::Memory(c) => c.seek(pos),
        }
    }
}

/// 音源のディレクトリ指定。設定で明示されていればそれを使う。
/// 埋め込みビルドでも、指定があればファイルから読む。
static OVERRIDE: OnceLock<Option<PathBuf>> = OnceLock::new();

fn asset_dir_override() -> Option<PathBuf> {
    OVERRIDE.get().cloned().flatten()
}

/// 起動時に一度だけ呼ぶ。
pub fn configure(cfg: &Config) {
    let _ = OVERRIDE.set(cfg.paths.assets());
}

fn asset_path(stem: &str) -> PathBuf {
    asset_dir().join(format!("{stem}.wav"))
}

/// 使う音源ディレクトリ。一度決めたら変えない。
///
/// 明示指定がなければ、本番音源が揃っているかを見て自動で選ぶ。
/// つくよみちゃんの音源を `assets/` に置いた時点で、何もしなくても
/// そちらに切り替わる。合成音を既定にベタ書きすると、本番音源を
/// 置いたあとも合成音が鳴り続けることになる。
fn asset_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        if let Some(dir) = asset_dir_override() {
            return dir;
        }
        let main = PathBuf::from("assets");
        if complete(&main) {
            return main;
        }
        let reference = main.join("reference");
        if complete(&reference) {
            eprintln!("  本番音源が無いので合成音を使う: {}", reference.display());
            return reference;
        }
        // どちらも欠けている。check_assets に本番側のパスで報告させる。
        main
    })
}

fn complete(dir: &Path) -> bool {
    Cue::every()
        .iter()
        .all(|c| dir.join(format!("{}.wav", c.stem())).exists())
}

/// 音源をバイナリに埋め込む。ラズパイに1ファイル置くだけで動かせる。
///
///     cargo build --release --features embed-audio
///
/// `include_bytes!` はコンパイル時にファイルを要求するので、
/// 音源が揃うまではこのフィーチャーを有効にできない。
///
/// モデルのほうは `embed-model` で埋め込む。両方入れると
/// バイナリ1つで完結する。
#[cfg(feature = "embed-audio")]
macro_rules! table {
    // 素材の一覧はここ1箇所。ディレクトリだけ差し替えられるようにしてある。
    ($dir:literal) => {
        table!(@ $dir,
            "intro", "question", "tail", "tail-lead", "bridge", "interlude", "all",
            "red", "blue", "yellow", "green", "yellowgreen", "white", "black",
            "pink", "orange", "purple", "brown", "lightblue",
        )
    };
    (@ $dir:literal, $($name:literal),* $(,)?) => {
        &[$(($name, include_bytes!(
            concat!(env!("CARGO_MANIFEST_DIR"), $dir, $name, ".wav")
        ) as &'static [u8])),*]
    };
}

/// 埋め込んだ実体。**名前は手で並べる。**
///
/// `include_bytes!` はリテラルのパスしか受け付けないので、`Cue::stem()` の
/// ように導くことができない。実体が無ければここで落ちるが、素材を足して
/// 表に書き忘れるほうは下の検査で見る。
///
/// 本番音源が無い状態で `.app` を作りたいときのために、`embed-reference` で
/// 確認用の合成音を埋め込める。**実行時のフォールバック（`asset_dir()` が
/// `assets/reference/` に落ちる）は埋め込みには効かない。** `include_bytes!`
/// はファイルの有無を見られないので、こちらは手で切り替えるしかない。
#[cfg(all(feature = "embed-audio", not(feature = "embed-reference")))]
const FILES: &[(&str, &[u8])] = table!("/assets/");
#[cfg(feature = "embed-reference")]
const FILES: &[(&str, &[u8])] = table!("/assets/reference/");

/// const で使う文字列の等値。`==` は const で回せない。
#[cfg(feature = "embed-audio")]
const fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    const_for!(i in 0..a.len() => {
        if a[i] != b[i] {
            return false;
        }
    });
    true
}

/// **素材がすべて表に載っているか、コンパイル時に確かめる。**
///
/// 実体が無ければ `include_bytes!` がその場で落ちるので、ここで見るのは
/// 名前の取りこぼしだけ。色を足して上の表に書き忘れると、以前は鳴らそう
/// とした時点で初めて「埋め込まれていない素材」になっていた。
#[cfg(feature = "embed-audio")]
const _: () = {
    let every = Cue::every();
    const_for!(i in 0..Cue::COUNT => {
        let mut found = false;
        const_for!(j in 0..FILES.len() => {
            if str_eq(FILES[j].0, every[i].stem()) {
                found = true;
            }
        });
        assert!(found, "埋め込みの表に無い素材がある（audio.rs の FILES を見ること）");
    });
};

#[cfg(feature = "embed-audio")]
fn embedded(stem: &str) -> Option<&'static [u8]> {
    FILES.iter().find(|(n, _)| *n == stem).map(|(_, b)| *b)
}

// ---------------------------------------------------------------- 録音

/// 監視の刻み。
const TICK: Duration = Duration::from_millis(25);

/// 起動時に環境ノイズを測る回数。刻んで中央値を取る。
/// 連続した区間の平均だと、その瞬間の物音ひとつで跳ね上がる。
const FLOOR_CHUNKS: usize = 8;

/// 入力ストリームを開きっぱなしにして持ち回す。
///
/// 呼ばれるたびに開き直すと、CoreAudio / ALSA のデバイス初期化に
/// 数百ミリ秒かかり、その間の声が録れない。質問の直後こそ子どもが
/// 叫ぶ瞬間なので、ここを取りこぼすと第一声が丸ごと消える。
pub struct Ears {
    _stream: cpal::Stream,
    buf: Arc<Mutex<Vec<f32>>>,
    rate: u32,
    /// 環境ノイズの推定値。毎回の観測で更新する。
    floor: f32,
    cfg: Listen,
}

impl Ears {
    pub fn new(cfg: &Config) -> Result<Self> {
        let device = cpal::default_host()
            .default_input_device()
            .context("入力デバイスがない")?;
        let supported = device.default_input_config()?;
        let rate = supported.sample_rate().0;
        let channels = supported.channels() as usize;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();

        let buf = Arc::new(Mutex::new(Vec::<f32>::new()));
        let err = |e| eprintln!("入力ストリームのエラー: {e}");

        // チャンネルを混ぜてモノラルにしながら溜める。
        macro_rules! sink {
            ($t:ty, $conv:expr) => {{
                let buf = Arc::clone(&buf);
                let conv: fn($t) -> f32 = $conv;
                device.build_input_stream(
                    &config,
                    move |data: &[$t], _: &cpal::InputCallbackInfo| {
                        let mut b = buf.lock().unwrap();
                        for frame in data.chunks(channels) {
                            b.push(frame.iter().map(|&s| conv(s)).sum::<f32>() / channels as f32);
                        }
                    },
                    err,
                    None,
                )?
            }};
        }

        let stream = match format {
            cpal::SampleFormat::F32 => sink!(f32, |s| s),
            cpal::SampleFormat::I16 => sink!(i16, |s| s as f32 / i16::MAX as f32),
            cpal::SampleFormat::U16 => sink!(u16, |s| (s as f32 - 32768.0) / 32768.0),
            other => anyhow::bail!("未対応のサンプル形式: {other:?}"),
        };
        stream.play()?;

        // 環境ノイズを測ってしきい値を決める。部屋の静かさは
        // 環境によって桁が違うので、固定値だと誤発火か取りこぼしになる。
        //
        // 刻んで中央値を取る。連続した区間の平均だと、測っている最中に
        // 物音がひとつ入るだけで倍以上ずれる。
        let mut levels = Vec::with_capacity(FLOOR_CHUNKS);
        for _ in 0..FLOOR_CHUNKS {
            buf.lock().unwrap().clear();
            std::thread::sleep(TICK * 2);
            levels.push(rms(&buf.lock().unwrap()));
        }
        levels.sort_by(|a, b| a.total_cmp(b));
        let floor = levels[FLOOR_CHUNKS / 2];
        let ears = Self {
            _stream: stream,
            buf,
            rate,
            floor,
            cfg: cfg.listen.clone(),
        };
        eprintln!("  環境ノイズ {floor:.4} → しきい値 {:.4}", ears.threshold());
        Ok(ears)
    }

    fn threshold(&self) -> f32 {
        if self.cfg.threshold > 0.0 {
            return self.cfg.threshold;
        }
        (self.floor * self.cfg.speech_ratio).clamp(self.cfg.speech_floor, self.cfg.speech_ceil)
    }

    /// 環境ノイズの推定を観測で更新する。
    ///
    /// 起動時の一発勝負にすると、その瞬間たまたま騒がしかっただけで
    /// セッション全体が聞こえなくなる。実際に上限に張り付いた。
    ///
    /// 静かになったら即座に追従し、うるさくなったらゆっくり上げる。
    /// 逆にすると、話し声を環境ノイズと誤認して自分の首を絞める。
    fn update_floor(&mut self, quietest: f32) {
        self.floor = if quietest < self.floor {
            quietest
        } else {
            self.floor * 0.9 + quietest * 0.1
        };
    }

    /// 応答を待つ。16kHz モノラルの f32 を返す。
    ///
    /// `max` は**上限**であって固定長ではない。声が途切れたら早く返す。
    /// 毎回待ち切ると歌の流れが死ぬ。
    ///
    /// 何も聞こえなければ `None`。呼び出し側でランダムな色に倒す。
    pub fn listen(&mut self, max: Duration) -> Result<Option<Vec<f32>>> {
        // ストリームは鳴っている間も回り続けているので、直前に流した
        // 歌が溜まっている。窓を開ける前に捨てる。
        self.buf.lock().unwrap().clear();

        let window = (self.rate as usize / 5).max(1);
        let start = Instant::now();
        let mut voiced = Duration::ZERO;
        let mut heard = false;
        let mut quiet_since: Option<Instant> = None;
        // しきい値を決める材料。声が届いていたのに拾えなかったのか、
        // そもそも何も鳴っていなかったのかを切り分けるために出す。
        let mut loudest = 0.0f32;
        let mut quietest = f32::MAX;
        let threshold = self.threshold();

        while start.elapsed() < max {
            std::thread::sleep(TICK);
            let level = {
                let b = self.buf.lock().unwrap();
                rms(&b[b.len().saturating_sub(window)..])
            };

            // 窓を開けた直後はスピーカーの残響が残っている。
            // ここで「聞こえた」と判定すると即座に打ち切ってしまう。
            if start.elapsed() < self.cfg.guard() {
                continue;
            }
            loudest = loudest.max(level);
            quietest = quietest.min(level);

            if level > threshold {
                // 単発のノイズで発火しないよう、続いた時間を見る。
                voiced += TICK;
                if voiced >= self.cfg.min_speech() {
                    heard = true;
                }
                quiet_since = None;
            } else {
                voiced = Duration::ZERO;
                if heard {
                    let since = *quiet_since.get_or_insert_with(Instant::now);
                    if since.elapsed() >= self.cfg.hangover() {
                        break;
                    }
                }
            }
        }

        if quietest < f32::MAX {
            self.update_floor(quietest);
        }
        eprintln!(
            "  {} （{:.1}秒待った / 最大 {:.4} vs しきい値 {:.4} → 次回 {:.4}）",
            if heard { "聞こえた" } else { "無言" },
            start.elapsed().as_secs_f32(),
            loudest,
            threshold,
            self.threshold()
        );
        if !heard {
            return Ok(None);
        }
        let raw = self.buf.lock().unwrap().clone();
        Ok(Some(resample(&raw, self.rate, WHISPER_SR)))
    }
}

/// 二乗平均平方根。ピーク値だと単発のノイズに引っ張られる。
fn rms(xs: &[f32]) -> f32 {
    if xs.is_empty() {
        return 0.0;
    }
    (xs.iter().map(|x| x * x).sum::<f32>() / xs.len() as f32).sqrt()
}

/// 線形補間でリサンプルする。音声認識に渡すだけなのでこれで足りる。
fn resample(src: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || src.is_empty() {
        return src.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let len = (src.len() as f64 / ratio) as usize;
    (0..len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let j = pos as usize;
            let frac = (pos - j as f64) as f32;
            let a = src[j];
            let b = *src.get(j + 1).unwrap_or(&a);
            a + (b - a) * frac
        })
        .collect()
}

/// 素材が揃っているか起動時に確かめる。
///
/// 途中で足りないと気づくと、遊んでいる最中に落ちる。
/// 足りないものはまとめて出す。
pub fn check_assets(cfg: &Config) -> Result<()> {
    configure(cfg);
    let missing: Vec<&str> = Cue::every()
        .into_iter()
        .filter(|&c| Media::open(c).is_err())
        .map(|c| c.stem())
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    anyhow::bail!(
        "音源が足りない（{} / {}）: {}",
        missing.len(),
        Cue::every().len(),
        missing.join(", ")
    )
}
