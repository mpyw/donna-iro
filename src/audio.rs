//! 録音と再生。
//!
//! cpal も rodio もクロスプラットフォームで、macOS では CoreAudio、
//! Raspberry Pi では ALSA を裏で使う。ソースは共通のままでよい。

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// whisper が受け取るサンプリングレート。
pub const WHISPER_SR: u32 = 16_000;

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

    /// 再生し終わるまで待つ。
    pub fn play(&self, path: &Path) -> Result<()> {
        let sink = rodio::Sink::try_new(&self.handle)?;
        let file = File::open(path).with_context(|| format!("音源がない: {}", path.display()))?;
        sink.append(rodio::Decoder::new(BufReader::new(file))?);
        sink.sleep_until_end();
        Ok(())
    }
}

/// 無音とみなす振幅。マイクのノイズフロアより上、囁き声より下を狙う。
const SILENCE: f32 = 0.02;
/// 声が途切れてから打ち切るまでの猶予。
const HANGOVER: Duration = Duration::from_millis(600);

/// 応答を待って録音する。16kHz モノラルの f32 を返す。
///
/// `max` は**上限**であって固定長ではない。声が途切れたら早く返す。
/// 毎回待ち切ると歌の流れが死ぬ。
///
/// 何も聞こえなければ `None`。呼び出し側でランダムな色に倒す。
pub fn listen(max: Duration) -> Result<Option<Vec<f32>>> {
    let device = cpal::default_host()
        .default_input_device()
        .context("入力デバイスがない")?;
    let supported = device.default_input_config()?;
    let sample_rate = supported.sample_rate().0;
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

    // 50ms ごとに直近 200ms の振幅を見て、声が途切れたら打ち切る。
    let window = (sample_rate as usize / 5).max(1);
    let start = Instant::now();
    let mut heard = false;
    let mut quiet_since: Option<Instant> = None;
    while start.elapsed() < max {
        std::thread::sleep(Duration::from_millis(50));
        let level = {
            let b = buf.lock().unwrap();
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
    drop(stream);

    if !heard {
        return Ok(None);
    }
    let raw = Arc::try_unwrap(buf)
        .map(|m| m.into_inner().unwrap())
        .unwrap_or_else(|a| a.lock().unwrap().clone());
    Ok(Some(resample(&raw, sample_rate, WHISPER_SR)))
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
