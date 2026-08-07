//! 進行。
//!
//! ```text
//! intro
//!   ↓
//! ┌→ question            どんないろがすき？（ト長調）
//! │    ↓ 最大5秒待つ（何も言わなければランダムな色）
//! │  <color>              その色の節（5小節・ト長調）
//! │    ↓
//! │  tail / tail-lead     節の最終小節。間奏へ向かうときだけ助走つき
//! │    ↓
//! │  bridge / interlude   3周に1回、交互に挟む
//! └──┘                    「ぜんぶ！」と言うまで無限ループ
//!      ↓「ぜんぶ！」
//!    finale               転調 → ぜんぶの節 → エンディング
//! ```
//!
//! 要点は**無反応にしないこと**。判定できなくても必ず何かを鳴らす。

use std::time::{Duration, Instant};

use anyhow::Result;

use crate::audio::Player;
use crate::color::Color;
use crate::cue::Cue;
use crate::listener::Listener;
use crate::matcher::{Answer, Matcher};
use crate::screen::{Frame, Screen};

/// 応答を待つ最大時間。2歳児は考えてから言うので短すぎると取りこぼす。
const LISTEN_MAX: Duration = Duration::from_secs(5);

/// 何周に1回、区切り（ブリッジまたは間奏）を挟むか。
const INSERT_EVERY: u32 = 3;

/// フィナーレで色を差し替える間隔。
const FLASH: Duration = Duration::from_millis(500);

pub struct Game {
    player: Player,
    screen: Box<dyn Screen>,
    matcher: Matcher,
    ears: Box<dyn Listener>,
}

impl Game {
    pub fn new(player: Player, ears: Box<dyn Listener>, screen: Box<dyn Screen>) -> Self {
        Self {
            player,
            screen,
            matcher: Matcher::new(),
            ears,
        }
    }

    pub fn run(&mut self) -> Result<()> {
        self.screen.show(Frame::Palette);
        self.player.play(Cue::Intro)?;

        // 「ぜんぶ！」と言うまで無限に続く。何度でも好きな色を
        // 答えられるのがこの遊びの本体なので、回数の上限は設けない。
        let mut round: u32 = 0;
        loop {
            round += 1;

            // まだ色が決まっていないので全色を出す。
            self.screen.show(Frame::Palette);

            // 質問は**鳴り止んだ時点で返る**。末尾の合いの手枠は
            // 無音のまま裏で流れ続け、そこが応答の窓になる。
            self.player.play_until_quiet(Cue::Question)?;

            let heard = self.ears.hear(LISTEN_MAX)?;
            let answer = heard.as_deref().and_then(|t| self.matcher.find(t));

            if answer == Some(Answer::All) {
                return self.finale();
            }

            // 聞き取れなければランダムな色。黙ってはいけない。
            // ここで All に倒してはならない。事故で終わってしまう。
            let color = match answer {
                Some(Answer::Color(c)) => c,
                _ => Color::random(),
            };
            self.screen.show(Frame::Single(color));
            self.player.play(Cue::Color(color))?;

            // 3周に1回、区切りを挟む。同じ質問と節の往復だけだと単調になる。
            // 挟むものはブリッジと間奏を交互に入れ替える。同じ区切りが
            // 毎回続くとそれ自体が単調になるため。
            let insert = round.is_multiple_of(INSERT_EVERY);
            let interlude_next = insert && (round / INSERT_EVERY).is_multiple_of(2);

            // 節の最終小節。間奏を launch する助走はアウフタクトで
            // この小節に属するので、間奏へ向かうときだけ差し替える。
            self.player.play(if interlude_next {
                Cue::TailLead
            } else {
                Cue::Tail
            })?;

            if insert {
                self.screen.show(Frame::Palette);
                self.player.play(if interlude_next {
                    Cue::Interlude
                } else {
                    Cue::Bridge
                })?;
            }
        }
    }

    /// 転調 → ぜんぶの節 → エンディング。
    ///
    /// 鳴らし終わるのを待つのではなく、全長を受け取って鳴っている間に
    /// 色を差し替える。「ぜんぶ」と答えたのだから、ぜんぶの色が
    /// 次々に出るほうが締めくくりらしい。
    fn finale(&mut self) -> Result<()> {
        let timing = self.player.begin(Cue::Finale)?;
        let end = Instant::now() + timing.total;
        while Instant::now() < end {
            self.screen.show(Frame::Single(Color::random()));
            let left = end.saturating_duration_since(Instant::now());
            std::thread::sleep(FLASH.min(left));
        }
        self.screen.show(Frame::Palette);
        Ok(())
    }
}
