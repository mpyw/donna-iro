//! 待たずに終わる実装。`--once`。

use crate::app::Control;

/// 待たずに終わる（`--once`）。フィナーレで止まる、以前の挙動。
pub struct Never;

impl Control for Never {
    /// `--once` は最後まで鳴らして終わる。途中でやめる口は持たない。
    fn stop_requested(&mut self) -> bool {
        false
    }

    fn wait_for_again(&mut self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_ends_immediately() {
        assert!(!Never.wait_for_again());
    }
}
