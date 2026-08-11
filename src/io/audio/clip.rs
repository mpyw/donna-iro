//! 復号済みの音。長さと末尾の無音を測るために一度メモリに載せる。

use std::time::Duration;

use anyhow::Result;
use rodio::Source;

use crate::app::cue::Cue;

use super::{assets::Media, SILENCE};

/// 復号済みの音。長さと末尾の無音を測るために一度メモリに載せる。
pub struct Clip {
    samples: Vec<f32>,
    channels: u16,
    rate: u32,
}

impl Clip {
    pub fn load(cue: Cue) -> Result<Self> {
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
