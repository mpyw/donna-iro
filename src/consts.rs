//! const 文脈で回すための小物。
//!
//! `const fn` の中では `for` が使えない。`Iterator::next` が const でない
//! ためで、書くと `E0015` で止まる。同じことを `while` で書けるが、添字の
//! 初期化と加算が毎回ついて回るうえ、**`i += 1` を書き忘れると const 評価が
//! 終わらなくなる**。範囲を渡す形にすれば、そこを間違えようがない。

/// `for $i in $range` の const 版。
///
/// ```ignore
/// for_range!(i in 0..N => {
///     out[i] = src[i];
/// });
/// ```
///
/// 範囲は一度だけ評価する。`0..s.len()` のように毎回計算し直したくない
/// ものを渡せるようにするため。
macro_rules! for_range {
    ($i:ident in $range:expr => $body:block) => {{
        let range = $range;
        let mut $i = range.start;
        while $i < range.end {
            $body
            $i += 1;
        }
    }};
}

pub(crate) use for_range;
