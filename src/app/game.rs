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
//!      ↓
//!    もう1回？             操作を待つ。押されたら intro へ戻る
//! ```
//!
//! 要点は**無反応にしないこと**。判定できなくても必ず何かを鳴らす。
//!
//! ただし**フィナーレのあとだけはポリシーが反転する**。あそこは遊びの
//! ループの外なので、無反応は「もう終わり」と読んでよい。ここで
//! ランダムに倒すと永久に終われなくなる。

use std::time::{Duration, Instant};

use anyhow::Result;
use strum::EnumCount;

use crate::app::color::{Answer, Color};
use crate::app::cue::Cue;
use crate::app::matcher::Matcher;
use crate::app::Config;
use crate::app::Control;
use crate::app::Listener;
use crate::app::Player;
use crate::app::{Frame, Screen};

pub struct Game {
    player: Box<dyn Player>,
    screen: Box<dyn Screen>,
    matcher: Matcher,
    ears: Box<dyn Listener>,
    control: Box<dyn Control>,
    listen_max: Duration,
    insert_every: u32,
    flash: Duration,
}

impl Game {
    pub fn new(
        player: Box<dyn Player>,
        ears: Box<dyn Listener>,
        screen: Box<dyn Screen>,
        control: Box<dyn Control>,
        cfg: &Config,
    ) -> Self {
        Self {
            player,
            screen,
            matcher: Matcher::new(cfg.recognize.head_segments),
            ears,
            control,
            listen_max: cfg.listen.max(),
            insert_every: cfg.game.insert_every,
            flash: cfg.game.flash(),
        }
    }

    /// ひと続き遊んで、そのあと「もう1回」を待つ。
    ///
    /// 待ちに声を使わないのは `control` 側に書いてある。ここで見るのは
    /// 「続けると言われたか」だけ。
    pub fn run(&mut self) -> Result<()> {
        loop {
            self.play_through()?;
            // 待っていることを画面に出してから待つ。黙って止まっていると、
            // 終わったのか固まったのか区別がつかない。
            self.screen.show(Frame::Again);
            if !self.control.wait() {
                return Ok(());
            }
        }
    }

    /// イントロから「ぜんぶ！」のフィナーレまで、ひと続き。
    ///
    /// 周回数はここで閉じているので、もう1回のたびに区切りの周期も
    /// 頭から数え直す。前回の続きから間奏が来ると唐突になる。
    fn play_through(&mut self) -> Result<()> {
        self.screen.show(Frame::palette());
        self.player.play(Cue::Intro)?;

        // 「ぜんぶ！」と言うまで無限に続く。何度でも好きな色を
        // 答えられるのがこの遊びの本体なので、回数の上限は設けない。
        let mut round: u32 = 0;
        loop {
            round += 1;

            // まだ色が決まっていないので全色を出す。
            self.screen.show(Frame::palette());

            // 質問は**鳴り止んだ時点で返る**。末尾の合いの手枠は
            // 無音のまま裏で流れ続け、そこが応答の窓になる。
            self.player.play_until_quiet(Cue::Question)?;

            let heard = self.ears.hear(self.listen_max)?;
            let answer = heard.as_deref().and_then(|t| self.matcher.find(t));

            if answer == Some(Answer::All) {
                return self.finale();
            }

            // 聞き取れなければランダムな色。黙ってはいけない。
            // ここで All に倒してはならない。事故で終わってしまう。
            let color = match answer {
                Some(Answer::Single(c)) => c,
                _ => Color::random(),
            };
            self.screen.show(Frame::Single(color));
            self.player.play(Cue::Color(color))?;

            // 3周に1回、区切りを挟む。同じ質問と節の往復だけだと単調になる。
            // 挟むものはブリッジと間奏を交互に入れ替える。同じ区切りが
            // 毎回続くとそれ自体が単調になるため。
            let insert = round.is_multiple_of(self.insert_every);
            let interlude_next = insert && (round / self.insert_every).is_multiple_of(2);

            // 節の最終小節。間奏を launch する助走はアウフタクトで
            // この小節に属するので、間奏へ向かうときだけ差し替える。
            self.player.play(if interlude_next {
                Cue::TailLead
            } else {
                Cue::Tail
            })?;

            if insert {
                self.screen.show(Frame::palette());
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
        let mut order = Color::ALL;
        while Instant::now() < end {
            order = shuffle(&order);
            self.screen.show(Frame::Palette(order));
            let left = end.saturating_duration_since(Instant::now());
            std::thread::sleep(self.flash.min(left));
        }
        self.screen.show(Frame::palette());
        Ok(())
    }
}

/// どの位置も前回と違う色になるように並べ替える。
///
/// 同じ場所が同じ色のままだと「入れ替わった」ように見えない。
/// 完全順列（derangement）になるまで引き直す。12色なら
/// 当たる確率が 1/e ≒ 37% なので、数回で決まる。
fn shuffle(prev: &[Color; Color::COUNT]) -> [Color; Color::COUNT] {
    use rand::seq::SliceRandom;
    let mut rng = rand::thread_rng();
    loop {
        let mut next = *prev;
        next.shuffle(&mut rng);
        if next.iter().zip(prev.iter()).all(|(a, b)| a != b) {
            return next;
        }
    }
}

#[cfg(test)]
mod tests {
    use strum::VariantArray;

    use super::*;

    use std::cell::RefCell;
    use std::rc::Rc;

    use crate::app::player::Timing;
    use crate::app::Config;

    /// 鳴らした順を記録するだけ。長さ0なので待ち時間が消える。
    #[derive(Clone, Default)]
    struct Tape(Rc<RefCell<Vec<Cue>>>);

    impl Player for Tape {
        fn play(&self, cue: Cue) -> Result<()> {
            self.0.borrow_mut().push(cue);
            Ok(())
        }
        fn play_until_quiet(&self, cue: Cue) -> Result<()> {
            self.play(cue)
        }
        fn begin(&self, cue: Cue) -> Result<Timing> {
            self.play(cue)?;
            Ok(Timing {
                total: Duration::ZERO,
                audible: Duration::ZERO,
            })
        }
    }

    /// 台本どおりに答える。`None` は「聞き取れなかった」。
    ///
    /// 台本が尽きたら「ぜんぶ」と答えて終わらせる。放っておくと
    /// ランダムな色で回り続けてテストが返らない。
    struct Script(std::vec::IntoIter<Option<&'static str>>);

    impl Listener for Script {
        fn hear(&mut self, _max: Duration) -> Result<Option<String>> {
            match self.0.next() {
                Some(answer) => Ok(answer.map(str::to_string)),
                None => Ok(Some("ぜんぶ".to_string())),
            }
        }
    }

    struct Blind;
    impl Screen for Blind {
        fn show(&mut self, _frame: Frame) {}
    }

    /// `n` 回だけ「もう1回」に応える。
    struct Again(usize);
    impl Control for Again {
        fn wait(&mut self) -> bool {
            let more = self.0 > 0;
            self.0 = self.0.saturating_sub(1);
            more
        }
    }

    /// 台本を渡して遊ばせ、鳴った順を返す。
    fn played(answers: &[Option<&'static str>], again: usize) -> Vec<Cue> {
        let tape = Tape::default();
        Game::new(
            Box::new(tape.clone()),
            Box::new(Script(answers.to_vec().into_iter())),
            Box::new(Blind),
            Box::new(Again(again)),
            &Config::default(),
        )
        .run()
        .unwrap();
        let cues = tape.0.borrow().clone();
        cues
    }

    #[test]
    fn answering_a_color_plays_its_phrase() {
        assert_eq!(
            played(&[Some("あか")], 0),
            [
                Cue::Intro,
                Cue::Question,
                Cue::Color(Color::Red),
                Cue::Tail,
                // 台本切れ →「ぜんぶ」
                Cue::Question,
                Cue::Finale,
            ]
        );
    }

    #[test]
    fn unheard_answer_still_plays_some_color() {
        let cues = played(&[None], 0);
        // 黙ってはいけない。ランダムな色に倒す。
        assert!(
            matches!(cues[2], Cue::Color(_)),
            "聞き取れなかったのに色が鳴っていない: {cues:?}"
        );
        // 事故で終わってはいけない。
        assert_ne!(cues[2], Cue::Finale);
    }

    #[test]
    fn a_break_comes_every_third_round() {
        let cues = played(&[Some("あか"); 6], 0);
        let breaks: Vec<Cue> = cues
            .iter()
            .filter(|c| matches!(c, Cue::Bridge | Cue::Interlude))
            .copied()
            .collect();
        // 6周で2回。ブリッジと間奏が交互に入る。
        assert_eq!(breaks, [Cue::Bridge, Cue::Interlude], "{cues:?}");
    }

    #[test]
    fn only_the_interlude_gets_a_run_up() {
        let cues = played(&[Some("あか"); 6], 0);
        // 助走（tail-lead）は間奏の直前だけ。ブリッジの前は素の tail。
        let lead = cues
            .iter()
            .position(|c| *c == Cue::TailLead)
            .expect("助走が無い");
        assert_eq!(cues[lead + 1], Cue::Interlude, "{cues:?}");
        assert_eq!(cues.iter().filter(|c| **c == Cue::TailLead).count(), 1);
    }

    #[test]
    fn again_replays_from_the_intro() {
        let cues = played(&[], 1);
        // 1周目 → もう1回 → 2周目。イントロから鳴らし直す。
        assert_eq!(
            cues,
            [
                Cue::Intro,
                Cue::Question,
                Cue::Finale,
                Cue::Intro,
                Cue::Question,
                Cue::Finale,
            ]
        );
    }

    #[test]
    fn shuffle_moves_every_position() {
        let mut order = Color::ALL;
        for _ in 0..200 {
            let next = shuffle(&order);
            // 全色が1つずつ残っている
            let mut a = next;
            a.sort_by_key(|c| c.stem());
            let mut b = Color::VARIANTS.to_vec();
            b.sort_by_key(|c| c.stem());
            assert_eq!(a.as_slice(), b, "色が増減している");
            // どの位置も色が変わっている
            assert!(
                next.iter().zip(order.iter()).all(|(x, y)| x != y),
                "同じ位置に同じ色が残った"
            );
            order = next;
        }
    }
}
