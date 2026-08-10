//! 「もう1回」をどう受け取るか。
//!
//! フィナーレのあと、遊びを続けるかどうかをここで待つ。**声では受けない。**
//! 歓声や物音で勝手に再開してしまうし、そもそも遊び終わったあとに
//! 「続ける」と決めるのは親の仕事なので、意思を持った操作に限る。
//!
//! マウス・キーボード・テレビのリモコンは、供給元が違うだけで意味は同じ。
//! だから口は1つにまとめてある。CEC のリモコン（evdev）を足すときも、
//! `Channel` の送り手をもう1本増やすだけで、ここから下は変わらない。
//!
//! `Listener`（色を答える）とは別系統にしてある。あちらに混ぜると、
//! フォールバックが「ぜんぶ」に倒れないよう型で塞いだ意図が崩れるため。

use std::io::Write;

/// もう1回遊ぶかを待つ。`false` なら終わり。
pub trait Control: Send {
    fn wait(&mut self) -> bool;
}

/// ウィンドウから届く合図を待つ。
///
/// **送り手が落ちること（= ウィンドウが閉じられた）が、そのまま「終わり」の
/// 合図になる。** 終了の経路を別に用意しなくてよい。`Frame` を送る側で
/// `Disconnected` をゲームの終了と読んでいるのと、向きが逆なだけで同じ作り。
///
/// 送り手は今のところウィンドウだけなので、その構成でしか作られない。
/// CEC のリモコンを足すときにこの `cfg` を外す。
#[cfg(feature = "window")]
pub struct Channel(pub std::sync::mpsc::Receiver<()>);

#[cfg(feature = "window")]
impl Control for Channel {
    fn wait(&mut self) -> bool {
        // 遊んでいる最中に押されたぶんは捨てる。子どもは歌っている間も
        // キーを叩くので、溜めたまま待ちに入るとフィナーレが鳴り終わった
        // 瞬間に再開してしまう。
        while self.0.try_recv().is_ok() {}
        self.0.recv().is_ok()
    }
}

/// 標準入力を1行読む。ターミナル構成用。
///
/// Enter で続き、EOF（Ctrl-D）で終わる。`Channel` と同じく
/// 「入力が絶えたら終わり」に揃えてある。
pub struct Stdin;

impl Control for Stdin {
    fn wait(&mut self) -> bool {
        print!("  もう1回？ [Enter=つづける / Ctrl-D=おわり] > ");
        if std::io::stdout().flush().is_err() {
            return false;
        }
        let mut line = String::new();
        // read_line が 0 を返すのは EOF のときだけ。空行でも改行ぶんの 1 は返る。
        matches!(std::io::stdin().read_line(&mut line), Ok(n) if n > 0)
    }
}

/// 待たずに終わる（`--once`）。フィナーレで止まる、以前の挙動。
pub struct Never;

impl Control for Never {
    fn wait(&mut self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_ends_immediately() {
        assert!(!Never.wait());
    }

    #[cfg(feature = "window")]
    #[test]
    fn channel_ends_when_the_sender_drops() {
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let mut c = Channel(rx);
        drop(tx); // ウィンドウが閉じられた
        assert!(!c.wait(), "送り手が居なくなったら終わり");
    }

    #[cfg(feature = "window")]
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
        assert!(!c.wait(), "溜まっていたぶんで再開してはいけない");
    }

    #[cfg(feature = "window")]
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
        assert!(c.wait(), "待っている間に押されたら続ける");
    }
}
