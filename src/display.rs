//! ターミナルに色の●を描く。
//!
//! PNG を用意してアスキーアートに変換する手もあるが、素材の管理と
//! 画像デコードの依存が増えるうえ、変換で色が濁る。丸は手で描ける
//! 形なので、ANSI の24bitカラーで直接打つ。
//!
//! 上半分ブロック「▀」の前景色と背景色に別々の色を入れると、
//! 1文字で縦2ピクセルぶん塗れる。ターミナルのセルは縦長なので、
//! これでピクセルがほぼ正方形になり、丸が丸く見える。

pub type Rgb = (u8, u8, u8);

const RESET: &str = "\x1b[0m";

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

fn paint(c: Rgb, on_edge: bool) -> Rgb {
    if on_edge {
        edge(c)
    } else {
        c
    }
}

fn cell(top: Option<Rgb>, bottom: Option<Rgb>) -> String {
    match (top, bottom) {
        (None, None) => " ".to_string(),
        (Some(t), None) => format!("\x1b[38;2;{};{};{}m▀{RESET}", t.0, t.1, t.2),
        (None, Some(b)) => format!("\x1b[38;2;{};{};{}m▄{RESET}", b.0, b.1, b.2),
        (Some(t), Some(b)) => format!(
            "\x1b[38;2;{};{};{}m\x1b[48;2;{};{};{}m▀{RESET}",
            t.0, t.1, t.2, b.0, b.1, b.2
        ),
    }
}

/// 色を横に並べて描く。半径 `r` はピクセル単位。
pub fn render(colors: &[Rgb], r: i32, gap: usize) -> String {
    let mut out = String::new();
    let mut y = -r;
    while y <= r {
        for (i, &c) in colors.iter().enumerate() {
            if i > 0 {
                out.push_str(&" ".repeat(gap));
            }
            for x in -r..=r {
                let t = hit(x, y, r).map(|e| paint(c, e));
                let b = hit(x, y + 1, r).map(|e| paint(c, e));
                out.push_str(&cell(t, b));
            }
        }
        out.push('\n');
        y += 2;
    }
    out
}

/// 1色を大きく。色のフレーズを歌っている間に出す。
pub fn one(c: Rgb) -> String {
    render(&[c], 9, 0)
}

/// 全色を横並びに。質問・区切り・「ぜんぶ」のときに出す。
pub fn all(colors: &[Rgb]) -> String {
    render(colors, 4, 1)
}
