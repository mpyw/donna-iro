//! テレビのリモコン（HDMI-CEC）から「もう1回」を受ける送り手。
//!
//! `Control` は実装しない。**`Channel` の送り手をもう1本増やすだけ**で、
//! 受け口から下は何も変わらない。マウス・キーボード・リモコンは供給元が
//! 違うだけで意味は同じ、という `app::control` の建前をそのまま守る。
//!
//! # なぜ minifb では受けられないか
//!
//! CEC のリモコンは、ラズパイでは `vc4-hdmi` ドライバが普通の入力デバイス
//! （`kbd` ハンドラ付き）として生やす。**ここまでは期待どおりなのだが、飛んで
//! くるキーが違う。** Bravia で実測すると:
//!
//! ```text
//! 決定  → KEY_OK (352)      ← これ
//! 上下左右 → KEY_UP / KEY_DOWN / KEY_LEFT / KEY_RIGHT
//! 戻る  → KEY_EXIT (174)
//! ```
//!
//! **`minifb::Key` に `OK` は無い。** ウィンドウ側が見ている `Space` と
//! `Enter` には一生ならないので、押しても無反応になる。だから evdev を
//! 直接読む。
//!
//! # 依存を足さない
//!
//! `input_event` は 24 バイトの固定長なので、`std::fs` で読んで自前で解けば
//! 済む。crate を1つ増やすほどの話ではない。
//!
//! **`cfg(target_os)` も使わない。** macOS には `/sys/class/input` が無く、
//! 走査が空を返して何もしないだけになる。`cfg` で切ると、その中身が
//! macOS 側の検査（`tools/check.sh`）を一度も通らなくなるので、コンパイルは
//! どこでも通す。

use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

/// `struct input_event` の大きさ。`timeval`（16）+ type（2）+ code（2）+
/// value（4）。64bit Linux の値で、ラズパイもこれ。
const EVENT_SIZE: usize = 24;
/// `type` と `code` と `value` の位置。
const TYPE_AT: usize = 16;
const CODE_AT: usize = 18;
const VALUE_AT: usize = 20;

/// `EV_KEY`。
const EV_KEY: u16 = 1;
/// 押した瞬間。1 が押下、0 が離上、2 が自動連打。
const PRESSED: i32 = 1;

/// 「もう1回」と読むキー。
///
/// **`KEY_OK` だけに絞らない。** CEC の UI コマンドをどのキーに割り当てるかは
/// カーネルのキーマップとテレビの実装しだいで、Bravia は `KEY_OK` だったが
/// 他社が `KEY_SELECT` や `KEY_ENTER` を出す余地はある。**どれで来ても同じ
/// 意味なので、広く受けて構わない。** 取りこぼすほうが損。
///
/// 逆に上下左右と `KEY_EXIT` は入れない。**押し間違いで再開させない。**
const AGAIN: &[u16] = &[
    352, // KEY_OK
    353, // KEY_SELECT
    28,  // KEY_ENTER
    96,  // KEY_KPENTER
    57,  // KEY_SPACE
];

/// 見張るデバイスの名前。
///
/// **`vc4-hdmi-0 HDMI Jack` は外す。** あれは抜き差しを報せるスイッチで、
/// キーは来ない。名前が前方一致なので、素朴に選ぶと拾ってしまう。
///
/// ここを増やせば他のハードにも広がる。USB のリモコンレシーバなどは名前が
/// 違うので、そのときに足す。
fn wanted(name: &str) -> bool {
    name.starts_with("vc4-hdmi") && !name.contains("Jack")
}

/// 1件ぶんの生データが「もう1回」の合図か。
///
/// **離上と自動連打は数えない。** 押している間ずっと真になると、
/// 長押し1回で何度も飛ぶ。
fn again_pressed(rec: &[u8]) -> bool {
    if rec.len() < EVENT_SIZE {
        return false;
    }
    let typ = u16::from_ne_bytes([rec[TYPE_AT], rec[TYPE_AT + 1]]);
    let code = u16::from_ne_bytes([rec[CODE_AT], rec[CODE_AT + 1]]);
    let value = i32::from_ne_bytes([
        rec[VALUE_AT],
        rec[VALUE_AT + 1],
        rec[VALUE_AT + 2],
        rec[VALUE_AT + 3],
    ]);
    typ == EV_KEY && value == PRESSED && AGAIN.contains(&code)
}

/// 見張る `/dev/input/eventN` を集める。
///
/// **読めなければ黙って空を返す。** リモコンが無くても遊びは成り立つ
/// （マウスとキーボードが残っている）ので、ここで止める理由が無い。
fn devices() -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir("/sys/class/input") else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for e in entries.flatten() {
        let node = e.file_name();
        let Some(node) = node.to_str() else { continue };
        if !node.starts_with("event") {
            continue;
        }
        let Ok(name) = std::fs::read_to_string(e.path().join("device/name")) else {
            continue;
        };
        if wanted(name.trim()) {
            found.push(PathBuf::from("/dev/input").join(node));
        }
    }
    // 名前の順に揃える。読む順に意味は無いが、ログが安定する。
    found.sort();
    found
}

/// 1つのデバイスを読み続ける。**送り先が閉じたら黙って終わる。**
fn watch(path: PathBuf, again: Sender<()>) {
    let Ok(mut file) = std::fs::File::open(&path) else {
        // 権限が無い（`input` グループに入っていない）ときにここへ来る。
        eprintln!("  ⚠ リモコンを読めない: {}", path.display());
        return;
    };
    let mut rec = [0u8; EVENT_SIZE];
    loop {
        // **1件ぶんきっちり読む。** evdev は件単位で届くので、途中で
        // 切れたら壊れている。read_exact が失敗したらそこで畳む。
        if file.read_exact(&mut rec).is_err() {
            return;
        }
        if again_pressed(&rec) && again.send(()).is_err() {
            // 待っている側がもう居ない = 遊びが終わった。
            return;
        }
    }
}

/// 見張りを立てる。**戻り値は見ているデバイスの数。**
///
/// 0 なら、リモコンからは何も来ない（CEC が繋がっていないか、権限が無いか、
/// Linux ではない）。**それでも遊びは続けられる**ので、呼び手は数を出す
/// だけでよい。
pub fn spawn(again: &Sender<()>) -> usize {
    let mut n = 0;
    for path in devices() {
        let tx = again.clone();
        std::thread::spawn(move || watch(path, tx));
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 生データを1件組む。テストのためだけの細工。
    fn event(typ: u16, code: u16, value: i32) -> [u8; EVENT_SIZE] {
        let mut rec = [0u8; EVENT_SIZE];
        rec[TYPE_AT..TYPE_AT + 2].copy_from_slice(&typ.to_ne_bytes());
        rec[CODE_AT..CODE_AT + 2].copy_from_slice(&code.to_ne_bytes());
        rec[VALUE_AT..VALUE_AT + 4].copy_from_slice(&value.to_ne_bytes());
        rec
    }

    /// Bravia の決定ボタンは `KEY_OK`。**実測した値そのもの。**
    #[test]
    fn the_tv_ok_button_counts() {
        assert!(again_pressed(&event(EV_KEY, 352, PRESSED)));
    }

    /// 他社が別のキーを出しても同じ意味に読む。
    #[test]
    fn other_ways_of_saying_ok_count_too() {
        for code in AGAIN {
            assert!(
                again_pressed(&event(EV_KEY, *code, PRESSED)),
                "code={code} を取りこぼしている"
            );
        }
    }

    /// **離上と連打は数えない。** 長押し1回で何度も飛ぶと、
    /// 待ちに入る前の掃除を抜けてしまう。
    #[test]
    fn only_the_moment_it_goes_down_counts() {
        assert!(!again_pressed(&event(EV_KEY, 352, 0)), "離上");
        assert!(!again_pressed(&event(EV_KEY, 352, 2)), "自動連打");
    }

    /// 十字キーと戻るで再開してはいけない。**押し間違いで戻される。**
    #[test]
    fn the_other_buttons_do_not_restart_the_game() {
        for code in [103u16, 105, 106, 108, 174, 158, 113, 114, 115] {
            assert!(
                !again_pressed(&event(EV_KEY, code, PRESSED)),
                "code={code} で再開してはいけない"
            );
        }
    }

    /// キー以外の種目は見ない（`EV_SYN` や `EV_MSC` が同じ口から来る）。
    #[test]
    fn ignores_events_that_are_not_keys() {
        assert!(!again_pressed(&event(0, 352, PRESSED)), "EV_SYN");
        assert!(!again_pressed(&event(4, 352, PRESSED)), "EV_MSC");
    }

    /// 短いものを渡されても落ちない。
    #[test]
    fn a_truncated_record_is_not_a_press() {
        assert!(!again_pressed(&[]));
        assert!(!again_pressed(&[0u8; EVENT_SIZE - 1]));
    }

    /// 抜き差しを報せるスイッチはキーを出さないので見張らない。
    #[test]
    fn the_hdmi_jack_switch_is_not_a_remote() {
        assert!(wanted("vc4-hdmi-0"));
        assert!(wanted("vc4-hdmi-1"));
        assert!(!wanted("vc4-hdmi-0 HDMI Jack"), "実際に名前が前方一致する");
        assert!(!wanted("pwr_button"));
        assert!(!wanted(""));
    }
}
