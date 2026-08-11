//! ウィンドウに丸を描く。
//!
//! ターミナルと違って実解像度で描けるので、丸はそのまま丸に出る。
//!
//! macOS ではウィンドウをメインスレッドに置く必要があり、しかも
//! `cpal::Stream` も `rodio::OutputStream` も `!Send` なので、
//! **ゲーム側をワーカースレッドに出してメインスレッドは描画に専念**する。
//! 再生や認識で固まらなくなる副次効果もある。

use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::OnceLock;

use anyhow::{Context, Result};
use minifb::{Key, KeyRepeat, MouseButton, Window, WindowOptions};

use crate::color::{Color, Rgb};
use crate::screen::{border, Frame, Screen};

const WIDTH: usize = 1280;
const HEIGHT: usize = 720;
/// 画面の更新間隔。音のほうが主役なので60fpsも要らない。
const FPS: u64 = 30;

/// ゲーム側が持つ手元の口。実際の描画はメインスレッドがやる。
pub struct Remote(pub Sender<Frame>);

impl Screen for Remote {
    fn show(&mut self, frame: Frame) {
        // 受け手が閉じていても、ゲームを止める理由にはならない。
        let _ = self.0.send(frame);
    }
}

/// メインスレッドで回す。ウィンドウが閉じられたら返る。
///
/// `again` は「もう1回」の合図を送る口。テレビの前にキーボードもマウスも
/// 無い前提なので、本命は CEC のリモコンだが、そちらは普通の入力デバイスと
/// して生えるぶんにはここに届く。届かない機種なら evdev を読む送り手を
/// 別に立てて、同じチャンネルに流す。
pub fn run(rx: Receiver<Frame>, again: Sender<()>) -> Result<()> {
    let mut window = Window::new(
        "どんないろがすき",
        WIDTH,
        HEIGHT,
        WindowOptions {
            resize: true,
            scale_mode: minifb::ScaleMode::AspectRatioStretch,
            ..WindowOptions::default()
        },
    )
    .context("ウィンドウを開けない")?;
    window.set_target_fps(FPS as usize);

    let mut buf = vec![0u32; WIDTH * HEIGHT];
    let mut frame = Frame::palette();
    let mut was_down = false;

    while window.is_open() && !window.is_key_down(Key::Escape) {
        // 溜まっているぶんは捨てて最新だけ描く。
        loop {
            match rx.try_recv() {
                Ok(f) => frame = f,
                Err(TryRecvError::Empty) => break,
                // ゲーム側が終わった（「もう1回」に応えが無かった、または落ちた）。
                // ウィンドウだけ残っても意味がないので畳む。
                Err(TryRecvError::Disconnected) => return Ok(()),
            }
        }

        // クリックは押した瞬間だけ拾う。get_mouse_down は押している間ずっと
        // 真なので、そのまま流すと 30fps ぶんの連打になる。キーのほうは
        // KeyRepeat::No で minifb が同じことをしてくれる。
        let down = window.get_mouse_down(MouseButton::Left);
        if (down && !was_down)
            || window.is_key_pressed(Key::Space, KeyRepeat::No)
            || window.is_key_pressed(Key::Enter, KeyRepeat::No)
        {
            // 待っていないときに送ったぶんは受け手が捨てる。ここでは
            // 遊びの最中かどうかを知らないので、素通しでよい。
            let _ = again.send(());
        }
        was_down = down;

        paint(&mut buf, frame);
        window.update_with_buffer(&buf, WIDTH, HEIGHT)?;
    }
    Ok(())
}

fn rgb(c: Rgb) -> u32 {
    ((c.0 as u32) << 16) | ((c.1 as u32) << 8) | c.2 as u32
}

fn paint(buf: &mut [u32], frame: Frame) {
    buf.fill(0x101010);
    match frame {
        Frame::Single(c) => {
            let r = (HEIGHT as f32 * 0.38) as i32;
            circle(buf, WIDTH as i32 / 2, HEIGHT as i32 / 2, r, c.rgb());
        }
        Frame::Palette(order) => palette(buf, &order, 1.0),
        Frame::Again => again(buf),
    }
}

/// 全色を並べる。`shade` で暗くできる。
fn palette(buf: &mut [u32], order: &[Color], shade: f32) {
    // 4列×3行。テレビで見たときに一番収まりがいい。
    let cols = 4usize;
    let rows = order.len().div_ceil(cols);
    let cw = WIDTH / cols;
    let ch = HEIGHT / rows;
    let r = (cw.min(ch) as f32 * 0.36) as i32;
    for (i, c) in order.iter().enumerate() {
        let cx = (i % cols) * cw + cw / 2;
        let cy = (i / cols) * ch + ch / 2;
        circle(buf, cx as i32, cy as i32, r, shaded(c.rgb(), shade));
    }
}

/// 「もう1回」を待っている画面。
///
/// **色は残したまま暗くして、真ん中にボタンを置く。** 遊びが終わったのでは
/// なく続きがある、と見えるようにするため。真っ暗にすると終了に見える。
///
/// 押すのは親（テレビならリモコン、PC ならマウス）なので、字で書くのが
/// 一番はっきりする。
///
/// なお**押すのは画面のどこでもよい**。これは押せることを示すボタンの絵で
/// あって、当たり判定ではない。テレビの前で正確に狙わせるのは無理がある。
fn again(buf: &mut [u32]) {
    palette(buf, &Color::ALL, 0.28);

    const LABEL: &str = "もう1回";
    let (cx, cy) = (WIDTH as i32 / 2, HEIGHT as i32 / 2);
    // 離れたテレビから読める大きさ。画面の高さに対して決める。
    let px = HEIGHT as f32 * 0.1;

    let w = text_width(LABEL, px);
    let (bw, bh) = (w + (px * 1.6) as i32, (px * 2.0) as i32);
    let r = (px * 0.4) as i32;

    // 暗い縁を先に敷く。後ろの白と黒の丸がボタンの端から覗いて、
    // どこまでがボタンなのか分からなくなるため。
    let pad = (px * 0.07) as i32;
    round_rect(
        buf,
        cx - bw / 2 - pad,
        cy - bh / 2 - pad,
        cx + bw / 2 + pad,
        cy + bh / 2 + pad,
        r + pad,
        (18, 18, 18),
    );
    round_rect(
        buf,
        cx - bw / 2,
        cy - bh / 2,
        cx + bw / 2,
        cy + bh / 2,
        r,
        (245, 245, 245),
    );
    text(buf, cx, cy, px, LABEL, (26, 26, 26));
}

/// 角の丸い長方形。ボタンに見せるためだけのもの。
fn round_rect(buf: &mut [u32], x0: i32, y0: i32, x1: i32, y1: i32, r: i32, c: Rgb) {
    let fill = rgb(c);
    for y in y0.max(0)..y1.min(HEIGHT as i32) {
        for x in x0.max(0)..x1.min(WIDTH as i32) {
            // 角の内側に丸を1つ置いて、そこから外れた画素だけ落とす。
            let nx = x.clamp(x0 + r, x1 - r);
            let ny = y.clamp(y0 + r, y1 - r);
            let (dx, dy) = (x - nx, y - ny);
            if dx * dx + dy * dy > r * r {
                continue;
            }
            buf[y as usize * WIDTH + x as usize] = fill;
        }
    }
}

/// 埋め込んだサブセットフォント。「もう1回」の4文字しか入っていない。
/// 出す文字を増やすときは `fonts/README.md` の手順で作り直すこと。
const FONT: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/fonts/again.ttf"));

/// 解析は一度きり。毎フレームやると読み込みのほうが描画より重くなる。
fn font() -> &'static fontdue::Font {
    static PARSED: OnceLock<fontdue::Font> = OnceLock::new();
    PARSED.get_or_init(|| {
        fontdue::Font::from_bytes(FONT, fontdue::FontSettings::default())
            .expect("埋め込んだフォントを読めない")
    })
}

fn text_width(s: &str, px: f32) -> i32 {
    let f = font();
    s.chars().map(|c| f.metrics(c, px).advance_width).sum::<f32>() as i32
}

/// 文字列を `(cx, cy)` を中心に描く。
///
/// 下地の色は問わない。ラスタライザが返す被覆率で混ぜるので、丸の上に
/// 重ねても縁がぎざつかない。
fn text(buf: &mut [u32], cx: i32, cy: i32, px: f32, s: &str, c: Rgb) {
    let f = font();
    let line = f.horizontal_line_metrics(px).expect("水平メトリクスが無い");
    // ascent は上、descent は下（負）。字面の中心が cy に来るようずらす。
    let baseline = cy + ((line.ascent + line.descent) / 2.0) as i32;

    let mut pen = cx - text_width(s, px) / 2;
    for ch in s.chars() {
        let (m, cov) = f.rasterize(ch, px);
        // ymin はベースラインから見た下端。画面は下が正なので符号が返る。
        let x0 = pen + m.xmin;
        let y0 = baseline - m.ymin - m.height as i32;
        for gy in 0..m.height {
            for gx in 0..m.width {
                let a = cov[gy * m.width + gx];
                if a == 0 {
                    continue;
                }
                let (x, y) = (x0 + gx as i32, y0 + gy as i32);
                if x < 0 || y < 0 || x >= WIDTH as i32 || y >= HEIGHT as i32 {
                    continue;
                }
                let i = y as usize * WIDTH + x as usize;
                let under = buf[i];
                let dst = ((under >> 16) as u8, (under >> 8) as u8, under as u8);
                buf[i] = rgb(mix(dst, c, a as f32 / 255.0));
            }
        }
        pen += m.advance_width as i32;
    }
}

/// `a` の重みで `to` を `from` に混ぜる。
fn mix(from: Rgb, to: Rgb, a: f32) -> Rgb {
    let m = |f: u8, t: u8| (f as f32 + (t as f32 - f as f32) * a) as u8;
    (m(from.0, to.0), m(from.1, to.1), m(from.2, to.2))
}

fn shaded(c: Rgb, f: f32) -> Rgb {
    let g = |v: u8| (v as f32 * f).clamp(0.0, 255.0) as u8;
    (g(c.0), g(c.1), g(c.2))
}

/// 塗りつぶした丸に、細い縁をつける。
/// 白と黒が背景に沈まないようにするため。
fn circle(buf: &mut [u32], cx: i32, cy: i32, r: i32, c: Rgb) {
    let fill = rgb(c);
    let edge = rgb(border(c));
    let rim = (r as f32 * 0.92) as i32;
    for y in (cy - r).max(0)..(cy + r).min(HEIGHT as i32) {
        for x in (cx - r).max(0)..(cx + r).min(WIDTH as i32) {
            let (dx, dy) = (x - cx, y - cy);
            let d2 = dx * dx + dy * dy;
            if d2 > r * r {
                continue;
            }
            buf[y as usize * WIDTH + x as usize] = if d2 > rim * rim { edge } else { fill };
        }
    }
}
