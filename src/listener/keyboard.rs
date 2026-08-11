//! 標準入力から色名を打つ `Listener`。音源と進行の確認用。

use std::io::Write;
use std::time::Duration;

use anyhow::Result;

use super::Listener;

/// 標準入力から色名を打つ。音源と進行の確認用。
pub struct Keyboard;

impl Listener for Keyboard {
    fn hear(&mut self, _max: Duration) -> Result<Option<String>> {
        print!("  いろは？ > ");
        std::io::stdout().flush()?;
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        let line = line.trim().to_string();
        Ok(if line.is_empty() { None } else { Some(line) })
    }
}
