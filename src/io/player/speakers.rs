//! スピーカーから鳴らす。rodio を使う。

use anyhow::{Context, Result};

use crate::app::cue::Cue;
use crate::app::player::Timing;
use crate::app::Player;
use crate::io::audio::Clip;

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
