//! マイクからの録音。cpal で開きっぱなしにして、呼ばれるたびに切り出す。

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::app::config::Listen;
use crate::app::Config;

use super::WHISPER_SR;

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
