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
pub struct Channel(pub std::sync::mpsc::Receiver<()>);

impl Control for Channel {
    fn wait_for_again(&mut self) -> bool {
        // 遊んでいる最中に押されたぶんは捨てる。子どもは歌っている間も
        // キーを叩くので、溜めたまま待ちに入るとフィナーレが鳴り終わった
        // 瞬間に再開してしまう。
        while self.0.try_recv().is_ok() {}
        self.0.recv().is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_ends_when_the_sender_drops() {
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let mut c = Channel(rx);
        drop(tx); // ウィンドウが閉じられた
        assert!(!c.wait_for_again(), "送り手が居なくなったら終わり");
    }

    #[test]
    fn channel_discards_what_piled_up_while_playing() {
        // 歌っている最中の連打が残ったままだと、フィナーレが鳴り終わった
        // 瞬間に再開してしまう。待ちに入る前のぶんは捨てること。
        let (tx, rx) = std::sync::mpsc::channel();
        let mut c = Channel(rx);
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
        let mut c = Channel(rx);
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
