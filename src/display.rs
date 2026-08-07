//! ターミナルに色の●を描く。
//!
//! 半角ブロック「▀」で縦2ピクセルを1文字に詰める手もあるが、フォントに
//! グリフが無いと代替フォントに落ちて高さが揃わず、縞模様に崩れる。
//!
//! ここでは**背景色を塗った空白**だけで描く。特殊なグリフを一切使わない
//! ので、どのターミナルでも同じに出る。ターミナルのセルは横1:縦2くらい
//! なので、1ピクセルを空白2つぶんの幅にすると丸が丸く見える。

pub type Rgb = (u8, u8, u8);

const RESET: &str = "\x1b[0m";
/// 1ピクセルの横幅（文字数）。セルが縦長なので2つ並べて正方形に近づける。
const PX: usize = 2;

/// 縁の色。白い●と黒い●が背景に溶けないよう、明るい色は暗く、
/// 暗い色は明るくずらしたリングを1ピクセル分描く。
fn edge(c: Rgb) -> Rgb {
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

/// (x, y) が円のどこか。None = 円の外、Some(true) = 縁、Some(false) = 内側。
fn hit(x: i32, y: i32, r: i32) -> Option<bool> {
    let d2 = x * x + y * y;
    if d2 > r * r {
        None
    } else {
        Some(d2 > (r - 1) * (r - 1))
    }
}

fn fill(c: Rgb) -> String {
    format!(
        "\x1b[48;2;{};{};{}m{}{RESET}",
        c.0,
        c.1,
        c.2,
        " ".repeat(PX)
    )
}

/// 色を横に並べて描く。半径 `r` はピクセル単位。
pub fn render(colors: &[Rgb], r: i32, gap: usize) -> String {
    let mut out = String::new();
    for y in -r..=r {
        for (i, &c) in colors.iter().enumerate() {
            if i > 0 {
                out.push_str(&" ".repeat(gap * PX));
            }
            for x in -r..=r {
                match hit(x, y, r) {
                    None => out.push_str(&" ".repeat(PX)),
                    Some(on_edge) => out.push_str(&fill(if on_edge { edge(c) } else { c })),
                }
            }
        }
        out.push('\n');
    }
    out
}

/// 1色を大きく。色のフレーズを歌っている間に出す。
pub fn one(c: Rgb) -> String {
    render(&[c], 7, 0)
}

/// 全色を横並びに。質問・区切り・「ぜんぶ」のときに出す。
pub fn all(colors: &[Rgb]) -> String {
    render(colors, 3, 1)
}
