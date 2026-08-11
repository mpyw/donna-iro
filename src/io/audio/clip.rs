//! 復号済みの音。長さと末尾の無音を測るために一度メモリに載せる。

use std::time::Duration;

use anyhow::Result;
use rodio::Source;

use crate::app::Cue;

use super::{assets::Media, SILENCE};

/// 復号済みの音。長さと末尾の無音を測るために一度メモリに載せる。
pub struct Clip {
    samples: Vec<f32>,
    channels: u16,
    rate: u32,
}

impl Clip {
    pub fn load(cue: Cue) -> Result<Self> {
        Self::decode(Media::open(cue)?)
    }

    /// 在処から離して復号する。**壊れた素材の扱いを装置なしで確かめるため。**
    /// 素材の検査が保証したいのは「復号できて、かつ音が入っている」で、
    /// それはここだけで決まる。
    fn decode<R>(src: R) -> Result<Self>
    where
        R: std::io::Read + std::io::Seek + Send + Sync + 'static,
    {
        let decoder = rodio::Decoder::new(src)?;
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

    pub fn total(&self) -> Duration {
        self.frames(self.samples.len() / self.channels as usize)
    }

    /// 末尾の無音を除いた長さ。
    pub fn audible(&self) -> Duration {
        let last = self
            .samples
            .iter()
            .rposition(|s| s.abs() > SILENCE)
            .map(|i| i / self.channels as usize + 1)
            .unwrap_or(0);
        self.frames(last)
    }

    pub fn into_source(self) -> rodio::buffer::SamplesBuffer<f32> {
        rodio::buffer::SamplesBuffer::new(self.channels, self.rate, self.samples)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 16bit モノラル 16kHz の wav を組む。`frames` が 0 ならヘッダだけ。
    fn wav(frames: u32) -> std::io::Cursor<Vec<u8>> {
        let (rate, bits, ch) = (16_000u32, 16u16, 1u16);
        let data = frames * u32::from(bits / 8) * u32::from(ch);
        let mut v = Vec::new();
        v.extend(b"RIFF");
        v.extend((36 + data).to_le_bytes());
        v.extend(b"WAVEfmt ");
        v.extend(16u32.to_le_bytes());
        v.extend(1u16.to_le_bytes()); // PCM
        v.extend(ch.to_le_bytes());
        v.extend(rate.to_le_bytes());
        v.extend((rate * u32::from(bits / 8) * u32::from(ch)).to_le_bytes());
        v.extend((bits / 8 * ch).to_le_bytes());
        v.extend(bits.to_le_bytes());
        v.extend(b"data");
        v.extend(data.to_le_bytes());
        // 無音でよい。ここで見たいのは長さがあるかどうか。
        v.extend(std::iter::repeat_n(0u8, data as usize));
        std::io::Cursor::new(v)
    }

    /// **素材の検査が保証したいのはこの2つ。** 開けるかどうかだけ見ていた頃は、
    /// 下の2つが起動時を素通りして、鳴らそうとした時点で初めて落ちていた。
    /// LFS を取り忘れたポインタ文字列もここに引っかかる。
    #[test]
    fn broken_or_empty_audio_is_not_a_clip() {
        // 空。
        assert!(Clip::decode(std::io::Cursor::new(Vec::new())).is_err());
        // wav ではない（LFS のポインタ文字列がまさにこれ）。
        let pointer = b"version https://git-lfs.github.com/spec/v1\noid sha256:0\n".to_vec();
        assert!(Clip::decode(std::io::Cursor::new(pointer)).is_err());

        // ヘッダは読めるが中身が無い。**復号は通ってしまう。**
        // 長さまで見ないと、これが素材として通る。
        let hollow = Clip::decode(wav(0)).expect("ヘッダは読める");
        assert!(hollow.total().is_zero(), "長さ0を長さ0と言えていない");

        // ちゃんとある。
        let good = Clip::decode(wav(16_000)).expect("普通の wav を読めない");
        assert_eq!(good.total(), Duration::from_secs(1));
    }
}
