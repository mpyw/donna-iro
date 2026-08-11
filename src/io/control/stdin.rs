//! 標準入力で待つ実装。ターミナル構成用。

use std::io::Write;

use crate::app::control::Control;

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
