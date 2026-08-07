//! ターミナルに色を出す。
//!
//! 丸で描こうとしたが、半径3では 1/5/5/7/5/5/1 のセル数にしかならず、
//! 円ではなく菱形になる。ターミナルの解像度で丸を出すのは無理があるので
//! 四角にしてある。
//!
//! 描画は**背景色を塗った空白**だけで行う。半角ブロック「▀」で縦2ピクセル
//! を1文字に詰める手もあるが、フォントにグリフが無いと代替に落ちて高さが
//! 揃わず縞に崩れる。空白なら特殊なグリフを使わないのでどこでも同じに出る。

pub type Rgb = (u8, u8, u8);

const RESET: &str = "\x1b[0m";
/// 1ピクセルの横幅（文字数）。セルが縦長なので2つ並べて正方形に近づける。
const PX: usize = 2;

/// 枠の色。白い■と黒い■が背景に溶けないよう、明るい色は暗く、
/// 暗い色は明るくずらした縁を1ピクセル分描く。
fn border(c: Rgb) -> Rgb {
    let lum = 0.299 * c.0 as f32 + 0.587 * c.1 as f32 + 0.114 * c.2 as f32;
    let f = |v: u8| {
        if lum < 100.0 {
            (v as f32 + 90.0).min(255.0) as u8
        } else {
            (v as f32 * 0.55) as u8
        }
    };
    (f(c.0), f(c.1), f(c.2))
}

fn pixel(c: Rgb) -> String {
    format!(
        "\x1b[48;2;{};{};{}m{}{RESET}",
        c.0,
        c.1,
        c.2,
        " ".repeat(PX)
    )
}

/// 色を横に並べた四角として描く。`size` はピクセル単位の一辺。
pub fn swatches(colors: &[Rgb], size: usize, gap: usize) -> String {
    let mut out = String::new();
    for y in 0..size {
        for (i, &c) in colors.iter().enumerate() {
            if i > 0 {
                out.push_str(&" ".repeat(gap * PX));
            }
            let edge_row = y == 0 || y + 1 == size;
            for x in 0..size {
                let edge = edge_row || x == 0 || x + 1 == size;
                out.push_str(&pixel(if edge { border(c) } else { c }));
            }
        }
        out.push('\n');
    }
    out
}

/// 1色を大きく。色のフレーズを歌っている間に出す。
pub fn one(c: Rgb) -> String {
    swatches(&[c], 12, 0)
}

/// 全色を横並びに。質問・区切り・「ぜんぶ」のときに出す。
pub fn all(colors: &[Rgb]) -> String {
    swatches(colors, 5, 1)
}

/// 画面。描くたびに消してから出す。
///
/// 流しっぱなしだと直前の色が上に残って、どれが今の色か分からなくなる。
/// 2歳児に見せるものなので、常に1つだけ映っている状態にする。
pub struct Screen;

impl Default for Screen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen {
    pub fn new() -> Self {
        Self
    }

    pub fn show(&mut self, art: &str) {
        use std::io::Write;
        print!("\x1b[2J\x1b[H\n{art}");
        let _ = std::io::stdout().flush();
    }
}
