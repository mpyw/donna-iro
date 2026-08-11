//! 標準入力から色名を打つ `Listener`。音源と進行の確認用。

use std::io::Write;
use std::time::Duration;

use anyhow::Result;

use crate::app::{Heard, Listener};

/// 標準入力から色名を打つ。音源と進行の確認用。
pub struct Keyboard;

impl Listener for Keyboard {
    fn hear(&mut self, _max: Duration) -> Result<Heard> {
        print!("  いろは？ > ");
        std::io::stdout().flush()?;
        let mut line = String::new();
        // 0 バイトは EOF。空行（改行だけ）とは別物で、こちらは終わり。
        if std::io::stdin().read_line(&mut line)? == 0 {
            println!();
            return Ok(Heard::Closed);
        }
        let line = line.trim();
        Ok(if line.is_empty() {
            Heard::Nothing
        } else {
            Heard::Said(line.to_string())
        })
    }
}
