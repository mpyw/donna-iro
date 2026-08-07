//! 録音と再生。
//!
//! cpal も rodio もクロスプラットフォームで、macOS では CoreAudio、
//! Raspberry Pi では ALSA を裏で使う。ソースは共通のままでよい。

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rodio::Source;

use crate::cue::Cue;

/// whisper が受け取るサンプリングレート。
pub const WHISPER_SR: u32 = 16_000;

/// これを下回れば無音とみなす振幅。再生側の末尾検出と録音側の
/// 打ち切り判定で同じ基準を使う。
const SILENCE: f32 = 0.02;

// ---------------------------------------------------------------- 再生

/// 出力ストリームを持ち回す。再生のたびに開き直すと
/// デバイスの初期化で無視できない間が空く。
pub struct Player {
    _stream: rodio::OutputStream,
    handle: rodio::OutputStreamHandle,
}

impl Player {
    pub fn new() -> Result<Self> {
        let (_stream, handle) =
            rodio::OutputStream::try_default().context("出力デバイスを開けない")?;
        Ok(Self { _stream, handle })
    }

    /// 鳴らし終わるまで待つ。
    pub fn play(&self, cue: Cue) -> Result<()> {
        self.start(cue, false)
    }

    /// **音が鳴り止んだ時点で返す。** 末尾の無音は裏で流したままにする。
    ///
    /// `question.wav` の末尾には原曲の合いの手枠が約1秒ぶん無音で入って
    /// いる。そこがまさに子どもが答える瞬間なので、鳴らし切ってから
    /// 聞き始めると完全に手遅れになる。
    ///
    /// 無音の長さは素材から測る。決め打ちにすると、つくよみちゃんの音源に
    /// 差し替えたときに合わなくなる。
    pub fn play_until_quiet(&self, cue: Cue) -> Result<()> {
        self.start(cue, true)
    }

    fn start(&self, cue: Cue, early: bool) -> Result<()> {
        let clip = Clip::load(cue)?;
        let wait = if early { clip.audible() } else { clip.total() };
        let sink = rodio::Sink::try_new(&self.handle)?;
        sink.append(clip.into_source());
        std::thread::sleep(wait);
        // 残り（末尾の無音）は裏で流し切らせる。
        sink.detach();
        Ok(())
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
    #[cfg(feature = "embed")]
    Memory(std::io::Cursor<&'static [u8]>),
}

impl Media {
    fn open(cue: Cue) -> Result<Self> {
        let stem = cue.stem();

        // 埋め込みがあり、かつディレクトリ指定で上書きされていなければ
        // バイナリの中から鳴らす。
        #[cfg(feature = "embed")]
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
            #[cfg(feature = "embed")]
            Media::Memory(c) => c.read(buf),
        }
    }
}

impl Seek for Media {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        match self {
            Media::File(f) => f.seek(pos),
            #[cfg(feature = "embed")]
            Media::Memory(c) => c.seek(pos),
        }
    }
}

/// 音源のディレクトリ指定。埋め込みビルドでもこれが設定されていれば
/// ファイルから読む。合成音での確認に使う。
///
///     DONNA_IRO_ASSETS=assets/reference cargo run
fn asset_dir_override() -> Option<String> {
    std::env::var("DONNA_IRO_ASSETS").ok()
}

fn asset_path(stem: &str) -> PathBuf {
    let dir = asset_dir_override().unwrap_or_else(|| "assets".to_string());
    PathBuf::from(dir).join(format!("{stem}.wav"))
}

/// 音源をバイナリに埋め込む。ラズパイに1ファイル置くだけで動かせる。
///
///     cargo build --release --features embed
///
/// `include_bytes!` はコンパイル時にファイルを要求するので、
/// 音源が揃うまではこのフィーチャーを有効にできない。
///
/// なお whisper のモデルは 141MB あるので埋め込まない。別ファイルのまま
/// `DONNA_IRO_MODEL` で場所を指す。
#[cfg(feature = "embed")]
fn embedded(stem: &str) -> Option<&'static [u8]> {
    macro_rules! table {
        ($($name:literal),* $(,)?) => {
            &[$(($name, include_bytes!(
                concat!(env!("CARGO_MANIFEST_DIR"), "/assets/", $name, ".wav")
            ) as &'static [u8])),*]
        };
    }
    const FILES: &[(&str, &[u8])] = table![
        "intro",
        "question",
        "tail",
        "tail-lead",
        "bridge",
        "interlude",
        "all",
        "red",
        "blue",
        "yellow",
        "green",
        "yellowgreen",
        "white",
        "black",
        "pink",
        "orange",
        "purple",
        "brown",
        "lightblue",
    ];
    FILES.iter().find(|(n, _)| *n == stem).map(|(_, b)| *b)
}

// ---------------------------------------------------------------- 録音

/// 声が途切れてから打ち切るまでの猶予。
/// 短くするほど反応が速くなるが、言い淀みで切れやすくなる。
const HANGOVER: Duration = Duration::from_millis(350);

/// 入力ストリームを開きっぱなしにして持ち回す。
///
/// 呼ばれるたびに開き直すと、CoreAudio / ALSA のデバイス初期化に
/// 数百ミリ秒かかり、その間の声が録れない。質問の直後こそ子どもが
/// 叫ぶ瞬間なので、ここを取りこぼすと第一声が丸ごと消える。
pub struct Ears {
    _stream: cpal::Stream,
    buf: Arc<Mutex<Vec<f32>>>,
    rate: u32,
}

impl Ears {
    pub fn new() -> Result<Self> {
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

        Ok(Self {
            _stream: stream,
            buf,
            rate,
        })
    }

    /// 応答を待つ。16kHz モノラルの f32 を返す。
    ///
    /// `max` は**上限**であって固定長ではない。声が途切れたら早く返す。
    /// 毎回待ち切ると歌の流れが死ぬ。
    ///
    /// 何も聞こえなければ `None`。呼び出し側でランダムな色に倒す。
    pub fn listen(&self, max: Duration) -> Result<Option<Vec<f32>>> {
        // ストリームは鳴っている間も回り続けているので、直前に流した
        // 歌が溜まっている。窓を開ける前に捨てる。
        self.buf.lock().unwrap().clear();

        // 25ms ごとに直近 200ms の振幅を見て、声が途切れたら打ち切る。
        let window = (self.rate as usize / 5).max(1);
        let start = Instant::now();
        let mut heard = false;
        let mut quiet_since: Option<Instant> = None;
        while start.elapsed() < max {
            std::thread::sleep(Duration::from_millis(25));
            let level = {
                let b = self.buf.lock().unwrap();
                let tail = &b[b.len().saturating_sub(window)..];
                tail.iter().fold(0.0f32, |m, s| m.max(s.abs()))
            };
            if level > SILENCE {
                heard = true;
                quiet_since = None;
            } else if heard {
                let since = *quiet_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= HANGOVER {
                    break;
                }
            }
        }

        if !heard {
            return Ok(None);
        }
        let raw = self.buf.lock().unwrap().clone();
        Ok(Some(resample(&raw, self.rate, WHISPER_SR)))
    }
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
pub fn check_assets() -> Result<()> {
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
