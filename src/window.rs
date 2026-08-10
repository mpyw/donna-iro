//! ウィンドウに丸を描く。
//!
//! ターミナルと違って実解像度で描けるので、丸はそのまま丸に出る。
//!
//! macOS ではウィンドウをメインスレッドに置く必要があり、しかも
//! `cpal::Stream` も `rodio::OutputStream` も `!Send` なので、
//! **ゲーム側をワーカースレッドに出してメインスレッドは描画に専念**する。
//! 再生や認識で固まらなくなる副次効果もある。

use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use anyhow::{Context, Result};
use minifb::{Key, KeyRepeat, MouseButton, Window, WindowOptions};

use crate::screen::{border, Frame, Rgb, Screen};

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
        Frame::Palette(order) => {
            // 4列×3行。テレビで見たときに一番収まりがいい。
            let cols = 4usize;
            let rows = order.len().div_ceil(cols);
            let cw = WIDTH / cols;
            let ch = HEIGHT / rows;
            let r = (cw.min(ch) as f32 * 0.36) as i32;
            for (i, c) in order.iter().enumerate() {
                let cx = (i % cols) * cw + cw / 2;
                let cy = (i / cols) * ch + ch / 2;
                circle(buf, cx as i32, cy as i32, r, c.rgb());
            }
        }
    }
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
