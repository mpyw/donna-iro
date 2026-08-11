//! マイクからの録音。cpal で開きっぱなしにして、呼ばれるたびに切り出す。

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::config::{Config, Listen};

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
    /// 入力ストリームが壊れた事実。一度立ったら下げない。
    ///
    /// **ログに出すだけでは足りない。** マイクを抜かれてもストリームは
    /// 黙って無音を流し続けるので、`listen` は毎回「無言」で返り、遊びは
    /// 永久にランダムな色を出し続ける。壊れているのに動いているように
    /// 見えるので、終了コードを直しても再起動の合図が立たない。
    fault: Arc<Mutex<Option<String>>>,
}

impl Ears {
    pub fn new(cfg: &Config) -> Result<Self> {
        let device = input_device(&cfg.listen)?;
        let supported = device.default_input_config()?;
        let rate = supported.sample_rate().0;
        eprintln!(
            "  マイク {}（{} ch / {} Hz）",
            device.name().unwrap_or_else(|_| "?".into()),
            supported.channels(),
            rate
        );
        let channels = supported.channels() as usize;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();

        let buf = Arc::new(Mutex::new(Vec::<f32>::new()));

        // **溜めっぱなしにしない。** ストリームは開きっぱなしなので、
        // listen() が呼ばれない間も溜まり続ける。「もう1回」の待ちは
        // 押されるまで無期限なので、上限が無いと際限なく増える。
        // 16kHz でも毎時 230MB、48kHz なら 0.7GB。テレビに繋ぎっぱなしの
        // 玩具でそれをやると、朝には起きてこない。
        //
        // listen() は頭で捨ててから聞くので、待ちの間のぶんは要らない。
        let cap = (rate as f32 * (cfg.listen.max().as_secs_f32() + 2.0)) as usize;

        let fault = Arc::new(Mutex::new(None::<String>));

        // **サンプルの型ごとに呼び分ける。** クロージャで束ねようとすると
        // 最初の型に固定されてしまうので、関数をそのまま呼ぶ。
        let sink = Sink {
            channels,
            cap,
            buf: &buf,
            fault: &fault,
        };
        let stream = match format {
            cpal::SampleFormat::F32 => sink.open(&device, &config, |s: f32| s)?,
            cpal::SampleFormat::I16 => {
                sink.open(&device, &config, |s: i16| s as f32 / i16::MAX as f32)?
            }
            cpal::SampleFormat::U16 => {
                sink.open(&device, &config, |s: u16| (s as f32 - 32768.0) / 32768.0)?
            }
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
            lock(&buf).clear();
            std::thread::sleep(TICK * 2);
            levels.push(rms(&lock(&buf)));
        }
        levels.sort_by(|a, b| a.total_cmp(b));
        let floor = levels[FLOOR_CHUNKS / 2];
        let ears = Self {
            _stream: stream,
            buf,
            rate,
            floor,
            cfg: cfg.listen.clone(),
            fault,
        };
        eprintln!("  環境ノイズ {floor:.4} → しきい値 {:.4}", ears.threshold());
        // 測っている間に壊れていたらここで止める。起動時に死んでいる
        // マイクで開始して、最初の聞き取りまで気づかないのを避ける。
        ears.check_fault()?;
        Ok(ears)
    }

    pub fn threshold(&self) -> f32 {
        if self.cfg.threshold > 0.0 {
            return self.cfg.threshold;
        }
        (self.floor * self.cfg.speech_ratio).clamp(self.cfg.speech_floor, self.cfg.speech_ceil)
    }

    /// 入力ストリームが壊れていないか。**無言と故障は別のこと。**
    ///
    /// 壊れたまま「聞こえなかった」を返し続けると、遊びはランダムな色を
    /// 出し続けて正常に見える。終了コードを直しても再起動の合図が立たない。
    fn check_fault(&self) -> Result<()> {
        match lock(&self.fault).as_deref() {
            Some(e) => anyhow::bail!("マイクの入力ストリームが壊れている: {e}"),
            None => Ok(()),
        }
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
        self.check_fault()?;

        // ストリームは鳴っている間も回り続けているので、直前に流した
        // 歌が溜まっている。窓を開ける前に捨てる。
        lock(&self.buf).clear();

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
                let b = lock(&self.buf);
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
        // 先に結果を作ってから、**最後に**故障を見る。窓を開けている
        // 最中に抜かれると、この回は「無言」で返ってランダムな色が1つ
        // 鳴り、気づくのが次の聞き取りまで遅れる。
        //
        // 取り出しと変換のあとに置くのは、その間に来たぶんも拾うため。
        // 完全に消せる競走ではないが、窓は最小になる。
        let out = heard.then(|| {
            // clone してから resample すると二度写す。持っていく。
            let raw = std::mem::take(&mut *lock(&self.buf));
            resample(&raw, self.rate, WHISPER_SR)
        });
        self.check_fault()?;
        Ok(out)
    }
}

/// 入力ストリームを開くための持ち物。**サンプルの型だけが違う3通りを
/// 1箇所にまとめる。**
///
/// `build_input_stream` はデータのコールバックが型で決まるので、クロージャ
/// ひとつでは束ねられない（クロージャはジェネリックにできない）。型ごとに
/// 呼び分けるしかないが、渡すものはどれも同じなので、そちらを持たせる。
struct Sink<'a> {
    channels: usize,
    cap: usize,
    buf: &'a Arc<Mutex<Vec<f32>>>,
    fault: &'a Arc<Mutex<Option<String>>>,
}

impl Sink<'_> {
    /// チャンネルを混ぜてモノラルにしながら溜める。
    fn open<T>(
        &self,
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        conv: fn(T) -> f32,
    ) -> Result<cpal::Stream>
    where
        T: cpal::SizedSample + Send + 'static,
    {
        let (buf, channels, cap) = (Arc::clone(self.buf), self.channels, self.cap);
        // 壊れたら残す。読むのは listen() の入口と出口。
        let broke = Arc::clone(self.fault);
        Ok(device.build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let mut b = lock(&buf);
                for frame in data.chunks(channels) {
                    b.push(frame.iter().map(|&s| conv(s)).sum::<f32>() / channels as f32);
                }
                trim(&mut b, cap);
            },
            move |e| {
                eprintln!("入力ストリームのエラー: {e}");
                lock(&broke).get_or_insert_with(|| e.to_string());
            },
            None,
        )?)
    }
}

/// ロックを取る。**毒されていても続ける。**
///
/// 音声のコールバックは cpal のスレッドで回る。そこが何かで落ちると
/// ミューテックスが毒され、以後は `unwrap` するたびにゲーム側まで道連れに
/// なる。中身は録音した波形と、壊れた事実の控えでしかないので、途中で
/// 落ちた誰かが居ても、そのまま使って困るものではない。
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 溜まりすぎた先頭を捨てる。**新しいほうを残す。**
///
/// 上限の倍まで伸ばしてからまとめて捨てる。毎回きっちり `cap` に収めると、
/// そのたびに残り全体をずらすことになる。
fn trim(buf: &mut Vec<f32>, cap: usize) {
    if buf.len() > cap * 2 {
        let drop = buf.len() - cap;
        buf.drain(..drop);
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

/// 使う入力デバイスを決める。
///
/// **見つからなくても止めない。** テレビに繋ぎっぱなしの玩具なので、起動
/// しないほうが困る。会議アプリがデバイスを掴んでいたり、USB を挿し直して
/// 名前が変わったりで、指定が外れることは普通に起きる。
///
/// ただし**黙って落ちない。** 名前を指定したのに違うマイクで動いていて
/// 精度が出ない、というのが一番たちが悪い。見えているものを並べて出す。
fn input_device(cfg: &Listen) -> Result<cpal::Device> {
    let default = || {
        cpal::default_host()
            .default_input_device()
            .context("入力デバイスがない")
    };
    let Some(want) = cfg.device() else {
        return default();
    };
    // **一度の列挙で名前まで集める。** 二度目を回すと、そちらが失敗した
    // ときに既定があるのに落ちる（USB を抜き挿しした直後などに起きる）。
    // 列挙そのものができないときも、既定に任せて続ける。
    let mut found = None;
    let mut names: Vec<String> = Vec::new();
    match cpal::default_host().input_devices() {
        Ok(devices) => {
            for d in devices {
                let Ok(name) = d.name() else { continue };
                if found.is_none() && name.contains(want) {
                    found = Some(d);
                }
                names.push(name);
            }
        }
        Err(e) => eprintln!("  ⚠ 入力デバイスを列挙できない: {e}"),
    }
    if let Some(device) = found {
        return Ok(device);
    }

    eprintln!("  ⚠ 指定した入力デバイスが見つからない: {want}");
    eprintln!(
        "    見えているもの: {}",
        if names.is_empty() {
            "（無し）".to_string()
        } else {
            names.join(" / ")
        }
    );
    eprintln!("    OS の既定で続ける。精度が出ないときはこの行を疑うこと。");
    default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 発話の判定はこの値だけを見る。ここが狂うと、しきい値をどう振っても
    /// 取りこぼすか誤発火するかになる。
    #[test]
    fn rms_measures_loudness_not_peaks() {
        assert_eq!(rms(&[]), 0.0);
        assert_eq!(rms(&[0.0, 0.0]), 0.0);
        assert!((rms(&[1.0, -1.0]) - 1.0).abs() < 1e-6);
        // 単発の尖りはピークなら 1.0 になるが、均すと小さい。
        // ピークで見ていた頃はここでノイズに引っ張られていた。
        let spike = rms(&[0.0, 0.0, 0.0, 1.0]);
        assert!(spike < 0.51, "単発のノイズに引っ張られている: {spike}");
    }

    /// **上限が無かった頃は朝まで待つと数百MBになった。** ストリームは
    /// 開きっぱなしなので、「もう1回」を待っている間も溜まり続ける。
    #[test]
    fn the_buffer_stays_bounded_while_nobody_is_listening() {
        const CAP: usize = 100;
        let mut buf = Vec::new();
        // コールバックが刻んで積むのを真似る。
        for round in 0..1_000 {
            buf.extend((0..7).map(|i| (round * 7 + i) as f32));
            trim(&mut buf, CAP);
            assert!(buf.len() <= CAP * 2, "上限を超えた: {}", buf.len());
        }
        // 捨てるのは古いほう。最後に積んだ値が残っていること。
        assert_eq!(*buf.last().unwrap(), 6_999.0);

        // 一度に大量に来ても収まる。
        let mut burst: Vec<f32> = (0..10_000).map(|i| i as f32).collect();
        trim(&mut burst, CAP);
        assert_eq!(burst.len(), CAP);
        assert_eq!(burst[0], 9_900.0);
    }

    #[test]
    fn resample_keeps_the_signal_and_changes_the_length() {
        // 同じレートなら素通し。
        let same = resample(&[0.1, 0.2, 0.3], 16_000, 16_000);
        assert_eq!(same, [0.1, 0.2, 0.3]);
        // 空も落ちない。
        assert!(resample(&[], 48_000, 16_000).is_empty());
        // 48k → 16k は 1/3 の長さ。
        let down = resample(&[0.0; 300], 48_000, 16_000);
        assert_eq!(down.len(), 100);
        // 16k → 48k は3倍。線形補間なので端の値は保たれる。
        let up = resample(&[0.0, 1.0], 16_000, 48_000);
        assert_eq!(up.len(), 6);
        assert_eq!(up[0], 0.0);
    }
}
