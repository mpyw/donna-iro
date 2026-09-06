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

// `crate::app::` で来るものが `app` の外向きの顔。`Matcher` はここでしか
// 使わない裏方なので、再エクスポートせず兄弟のまま参照する。
use super::matcher::Matcher;
use crate::app::{Answer, Color, Control, Cue, Frame, Heard, Listener, Player, Screen};
use crate::config::Config;

/// 「やめる」を見に行く間隔。**歌の途中で抜けるので細かく見る。**
/// 30fps の描画より粗い刻みだと、押してから止まるまでが目に見える。
const POLL: Duration = Duration::from_millis(50);

pub struct Game {
    player: Box<dyn Player>,
    screen: Box<dyn Screen>,
    matcher: Matcher,
    listener: Box<dyn Listener>,
    control: Box<dyn Control>,
    listen_max: Duration,
    insert_every: u32,
    flash: Duration,
}

impl Game {
    pub fn new(
        player: Box<dyn Player>,
        listener: Box<dyn Listener>,
        screen: Box<dyn Screen>,
        control: Box<dyn Control>,
        cfg: &Config,
    ) -> Self {
        Self {
            player,
            screen,
            matcher: Matcher::new(cfg.recognize.head_segments),
            listener,
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
            if !self.play_through()? {
                // 入力が絶えた。「もう1回」を訊く相手がいない。
                return Ok(());
            }
            // 待っていることを画面に出してから待つ。黙って止まっていると、
            // 終わったのか固まったのか区別がつかない。
            self.screen.show(Frame::Again);
            if !self.control.wait_for_again() {
                return Ok(());
            }
        }
    }

    /// 鳴らして、鳴り終わるまで待つ。
    ///
    /// **待っている間に「やめる」を見る。** 来たら鳴っている音を止めて
    /// すぐ返る。止めないと画面だけ切り替わって歌が流れ続ける。
    ///
    /// `quiet` が真なら、末尾の無音を待たずに音が鳴り止んだ時点で返る。
    /// `question.wav` の合いの手枠がそこに入っていて、応答の窓になる。
    ///
    /// 戻り値は**遊びを続けてよいか**。
    fn sound(&mut self, cue: Cue, quiet: bool) -> Result<bool> {
        let timing = self.player.begin(cue)?;
        let end = Instant::now() + if quiet { timing.audible } else { timing.total };
        loop {
            if self.control.stop_requested() {
                self.player.silence();
                return Ok(false);
            }
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(true);
            }
            std::thread::sleep(POLL.min(left));
        }
    }

    /// イントロから「ぜんぶ！」のフィナーレまで、ひと続き。
    ///
    /// 周回数はここで閉じているので、もう1回のたびに区切りの周期も
    /// 頭から数え直す。前回の続きから間奏が来ると唐突になる。
    fn play_through(&mut self) -> Result<bool> {
        // **待っている間に押されたぶんは捨てる。** 「もう1回」の画面で
        // 戻るを押していたら、始めた瞬間に抜けてしまう。
        let _ = self.control.stop_requested();

        self.screen.show(Frame::palette());
        if !self.sound(Cue::Intro, false)? {
            return Ok(true);
        }

        // 「ぜんぶ！」と言うまで無限に続く。何度でも好きな色を
        // 答えられるのがこの遊びの本体なので、回数の上限は設けない。
        let mut round: u32 = 0;
        loop {
            round += 1;

            // まだ色が決まっていないので全色を出す。
            self.screen.show(Frame::palette());

            // 質問は**鳴り止んだ時点で返る**。末尾の合いの手枠は
            // 無音のまま裏で流れ続け、そこが応答の窓になる。
            if !self.sound(Cue::Question, true)? {
                return Ok(true);
            }

            let answer = match self.listener.hear(self.listen_max)? {
                Heard::Said(text) => {
                    // **判定を出すのはここだけ。** `Matcher` の中で書くと、
                    // 変異を掛けるテストが数千行のログを吐く。落選まで
                    // 出しておかないと、外したときに手掛かりが残らない。
                    //
                    // stderr へ直に書く。装置でもファイルでもないので、
                    // `app` の縛り（`app.rs` を見ること）には触れない。
                    let verdict = self.matcher.find(&text);
                    eprintln!("  {verdict}");
                    // **`None` は「測ってすらいない」だけ。** 遠かったものは
                    // 判定の側で一番近いものへ倒してある。足切りして黙るより、
                    // それっぽいものを鳴らすほうが当たる（`matcher.rs`）。
                    verdict.answer()
                }
                Heard::Nothing => None,
                Heard::Closed => return Ok(false),
            };

            // **聞き取りの間に押されたぶんもここで拾う。** 録音は止められ
            // ないので、抜けるのは窓が閉じたあと（最大 listen.max_seconds
            // ぶん遅れる）。歌の途中は上の `sound` が即座に抜ける。
            if self.control.stop_requested() {
                return Ok(true);
            }

            if answer == Some(Answer::All) {
                self.finale()?;
                return Ok(true);
            }

            // 何も聞こえなかったときだけランダムな色。黙ってはいけない。
            // **声が届いていれば、遠くてもここには来ない**（判定が一番近い
            // ものへ倒す）。ここに来るのは無言と、1音だけのときだけ。
            //
            // ここで All に倒してはならない。何も言っていないのに終わる。
            let color = match answer {
                Some(Answer::Single(c)) => c,
                _ => Color::random(),
            };
            self.screen.show(Frame::Single(color));
            if !self.sound(Cue::Color(color), false)? {
                return Ok(true);
            }

            // 3周に1回、区切りを挟む。同じ質問と節の往復だけだと単調になる。
            // 挟むものはブリッジと間奏を交互に入れ替える。同じ区切りが
            // 毎回続くとそれ自体が単調になるため。
            let insert = round.is_multiple_of(self.insert_every);
            let interlude_next = insert && (round / self.insert_every).is_multiple_of(2);

            // 節の最終小節。間奏を launch する助走はアウフタクトで
            // この小節に属するので、間奏へ向かうときだけ差し替える。
            let tail = if interlude_next {
                Cue::TailLead
            } else {
                Cue::Tail
            };
            if !self.sound(tail, false)? {
                return Ok(true);
            }

            if insert {
                self.screen.show(Frame::palette());
                let between = if interlude_next {
                    Cue::Interlude
                } else {
                    Cue::Bridge
                };
                if !self.sound(between, false)? {
                    return Ok(true);
                }
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
            // **フィナーレも途中で抜けられる。** 行き先は「もう1回」の
            // 画面で、最後まで鳴らした場合と同じなので、返り値は要らない。
            if self.control.stop_requested() {
                self.player.silence();
                break;
            }
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
    use std::cell::RefCell;
    use std::rc::Rc;

    use strum::VariantArray;

    use super::*;
    use crate::app::Timing;
    use crate::config::Config;

    /// 鳴らした順を記録するだけ。長さ0なので待ち時間が消える。
    #[derive(Clone, Default)]
    struct Tape {
        cues: Rc<RefCell<Vec<Cue>>>,
        /// 止めた回数。**画面だけ切り替えて鳴らし続ける事故を捕まえる。**
        silenced: Rc<RefCell<usize>>,
    }

    impl Player for Tape {
        fn begin(&self, cue: Cue) -> Result<Timing> {
            self.cues.borrow_mut().push(cue);
            Ok(Timing {
                total: Duration::ZERO,
                audible: Duration::ZERO,
            })
        }
        fn silence(&self) {
            *self.silenced.borrow_mut() += 1;
        }
    }

    /// 台本どおりに答える。`None` は「聞き取れなかった」。
    ///
    /// 台本が尽きたら「ぜんぶ」と答えて終わらせる。放っておくと
    /// ランダムな色で回り続けてテストが返らない。
    struct Script(std::vec::IntoIter<Option<&'static str>>);

    impl Listener for Script {
        fn hear(&mut self, _max: Duration) -> Result<Heard> {
            Ok(match self.0.next() {
                Some(Some(answer)) => Heard::Said(answer.to_string()),
                Some(None) => Heard::Nothing,
                // 台本が尽きたら「ぜんぶ」。放っておくとランダムな色で
                // 回り続けてテストが返らない。
                None => Heard::Said("ぜんぶ".to_string()),
            })
        }
    }

    struct Blind;
    impl Screen for Blind {
        fn show(&mut self, _frame: Frame) {}
    }

    /// `n` 回だけ「もう1回」に応える。やめるとは言わない。
    struct Again(usize);
    impl Control for Again {
        fn stop_requested(&mut self) -> bool {
            false
        }
        fn wait_for_again(&mut self) -> bool {
            let more = self.0 > 0;
            self.0 = self.0.saturating_sub(1);
            more
        }
    }

    /// `after` 回訊かれたところで「やめる」と言い、そのあと
    /// 「もう1回」には `again` 回だけ応える。
    struct StopAfter {
        after: usize,
        again: usize,
    }
    impl Control for StopAfter {
        fn stop_requested(&mut self) -> bool {
            if self.after == 0 {
                return false;
            }
            self.after -= 1;
            self.after == 0
        }
        fn wait_for_again(&mut self) -> bool {
            let more = self.again > 0;
            self.again = self.again.saturating_sub(1);
            more
        }
    }

    /// 台本を渡して遊ばせ、鳴った順を返す。
    fn played(answers: Vec<Option<&'static str>>, again: usize) -> Vec<Cue> {
        let tape = Tape::default();
        Game::new(
            Box::new(tape.clone()),
            Box::new(Script(answers.into_iter())),
            Box::new(Blind),
            Box::new(Again(again)),
            &Config::default(),
        )
        .run()
        .unwrap();
        let cues = tape.cues.borrow().clone();
        cues
    }

    /// **「戻る」で歌の途中から「もう1回」へ跳ぶ。** 終わりではないので、
    /// そのあと続けると言われればもう一周する。
    #[test]
    fn stopping_midway_jumps_to_the_again_screen() {
        let tape = Tape::default();
        Game::new(
            Box::new(tape.clone()),
            // 2周目は「ぜんぶ」で素直に畳ませる。1周目は台本まで届かない。
            Box::new(Script(vec![Some("ぜんぶ")].into_iter())),
            Box::new(Blind),
            // 3回目に訊かれたところでやめる。そのあと1回だけ続ける。
            Box::new(StopAfter { after: 3, again: 1 }),
            &Config::default(),
        )
        .run()
        .unwrap();

        // **質問の途中で抜けて、もう1周まわっている。** 終わりではなく
        // 「もう1回」の画面へ跳んだ、というのがこの並びの意味。
        assert_eq!(
            *tape.cues.borrow(),
            [
                Cue::Intro,
                Cue::Question, // ← ここで「戻る」
                Cue::Intro,    // ← 続けると言われて2周目
                Cue::Question,
                Cue::Finale,
            ]
        );
        // **鳴っている音を止めていること。** 止めないと画面だけ切り替わって
        // 歌が流れ続ける。
        assert_eq!(*tape.silenced.borrow(), 1, "止めていない");
    }

    #[test]
    fn answering_a_color_plays_its_phrase() {
        assert_eq!(
            played(vec![Some("あか")], 0),
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

    /// 入力そのものが閉じたら、そこで畳む。
    ///
    /// **`--keyboard` をパイプで流し込むと EOF がここに来る。** 「無言」と
    /// 同じ扱いにしていた頃は、ランダムな色を上限まで鳴らし続けて返らなかった。
    /// フィナーレも「もう1回」も通らずに終わるのが正しい。
    #[test]
    fn closed_input_ends_the_game_without_a_finale() {
        struct Eof;
        impl Listener for Eof {
            fn hear(&mut self, _max: Duration) -> Result<Heard> {
                Ok(Heard::Closed)
            }
        }
        let tape = Tape::default();
        Game::new(
            Box::new(tape.clone()),
            Box::new(Eof),
            Box::new(Blind),
            // 何度でも応える。**閉じた側が勝たないと返ってこない。**
            Box::new(Again(usize::MAX)),
            &Config::default(),
        )
        .run()
        .unwrap();

        let cues = tape.cues.borrow().clone();
        assert_eq!(
            cues,
            [Cue::Intro, Cue::Question],
            "余計に鳴っている: {cues:?}"
        );
        assert!(!cues.contains(&Cue::Finale), "閉じたのにフィナーレが鳴った");
    }

    #[test]
    fn unheard_answer_still_plays_some_color() {
        let cues = played(vec![None], 0);
        // 黙ってはいけない。ランダムな色に倒す。
        assert!(
            matches!(cues[2], Cue::Color(_)),
            "聞き取れなかったのに色が鳴っていない: {cues:?}"
        );
        // 事故で終わってはいけない。
        assert_ne!(cues[2], Cue::Finale);
    }

    /// 遠い声でも、**判定が倒した色がそのまま鳴ること。**
    ///
    /// 足切りしていた頃はここがランダムだったので、繋ぎ間違えても気づけない。
    /// 「あたん」は実機のログで、子どもは「あか」と言っていた（`matcher.rs`）。
    #[test]
    fn a_distant_answer_plays_the_closest_color() {
        let cues = played(vec![Some("あたん")], 0);
        assert_eq!(cues[2], Cue::Color(Color::Red), "{cues:?}");
    }

    #[test]
    fn a_break_comes_every_third_round() {
        let cues = played(vec![Some("あか"); 6], 0);
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
        let cues = played(vec![Some("あか"); 6], 0);
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
        let cues = played(vec![], 1);
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
