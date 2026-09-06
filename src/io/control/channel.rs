//! ウィンドウ（や将来の CEC リモコン）から届く合図で待つ実装。

use crate::app::Control;

/// ウィンドウから届く合図を待つ。
///
/// **送り手が落ちること（= ウィンドウが閉じられた）が、そのまま「終わり」の
/// 合図になる。** 終了の経路を別に用意しなくてよい。`Frame` を送る側で
/// `Disconnected` をゲームの終了と読んでいるのと、向きが逆なだけで同じ作り。
///
/// 送り手は今のところウィンドウだけなので、その構成でしか作られない。
/// CEC のリモコンを足すときにこの `cfg` を外す。
pub struct Channel {
    /// 「もう1回」。
    pub again: std::sync::mpsc::Receiver<()>,
    /// 「この回はやめる」。**別の口にしてある。** 同じ口に混ぜると、
    /// やめる合図で再開してしまう。
    pub stop: std::sync::mpsc::Receiver<()>,
}

impl Control for Channel {
    /// **待たない。** 溜まっているぶんは全部捨てて、1つでもあれば真。
    /// 同じ押下で二度抜けないようにするため。
    ///
    /// 送り手が落ちていても真にしない。**それは終了の合図で、こちらの
    /// 担当ではない**（`wait_for_again` が `false` を返して畳む）。
    fn stop_requested(&mut self) -> bool {
        let mut pressed = false;
        while self.stop.try_recv().is_ok() {
            pressed = true;
        }
        pressed
    }

    fn wait_for_again(&mut self) -> bool {
        // 遊んでいる最中に押されたぶんは捨てる。子どもは歌っている間も
        // キーを叩くので、溜めたまま待ちに入るとフィナーレが鳴り終わった
        // 瞬間に再開してしまう。
        while self.again.try_recv().is_ok() {}
        self.again.recv().is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 「もう1回」だけを見る `Channel`。やめる側は使わないので、
    /// 送り手を落とした空の口を渡す。
    fn only_again(again: std::sync::mpsc::Receiver<()>) -> Channel {
        let (_, stop) = std::sync::mpsc::channel();
        Channel { again, stop }
    }

    /// **やめる合図で再開してはいけない。** 同じ口に混ぜると起きる。
    #[test]
    fn stopping_and_continuing_are_different_mouths() {
        let (again_tx, again) = std::sync::mpsc::channel();
        let (stop_tx, stop) = std::sync::mpsc::channel();
        let mut c = Channel { again, stop };

        assert!(!c.stop_requested(), "何も来ていない");
        stop_tx.send(()).unwrap();
        stop_tx.send(()).unwrap();
        assert!(c.stop_requested(), "押されたら真");
        assert!(!c.stop_requested(), "溜まっていたぶんは捨てる");

        // 「もう1回」を送っても、やめる側は動かない。
        again_tx.send(()).unwrap();
        assert!(!c.stop_requested());
    }

    #[test]
    fn channel_ends_when_the_sender_drops() {
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let mut c = only_again(rx);
        drop(tx); // ウィンドウが閉じられた
        assert!(!c.wait_for_again(), "送り手が居なくなったら終わり");
    }

    #[test]
    fn channel_discards_what_piled_up_while_playing() {
        // 歌っている最中の連打が残ったままだと、フィナーレが鳴り終わった
        // 瞬間に再開してしまう。待ちに入る前のぶんは捨てること。
        let (tx, rx) = std::sync::mpsc::channel();
        let mut c = only_again(rx);
        for _ in 0..5 {
            tx.send(()).unwrap();
        }
        drop(tx);
        assert!(!c.wait_for_again(), "溜まっていたぶんで再開してはいけない");
    }

    #[test]
    fn channel_continues_when_pressed_while_waiting() {
        use std::time::Duration;

        let (tx, rx) = std::sync::mpsc::channel();
        let mut c = only_again(rx);
        // 待ちに入った瞬間は外から分からないので、しばらく押し続ける。
        // 最初の1回が掃除に巻き込まれても、次のどれかは必ず届く。
        std::thread::spawn(move || {
            for _ in 0..40 {
                if tx.send(()).is_err() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        });
        assert!(c.wait_for_again(), "待っている間に押されたら続ける");
    }
}
