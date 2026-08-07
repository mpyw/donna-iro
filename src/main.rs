//! 「どんな色がすき？」と呼びかけ、子どもが叫んだ色名を聞き取って
//! その色の続きを再生する。
//!
//!     cargo run                                       マイク＋ウィンドウ
//!     cargo run -- --terminal                         ウィンドウを使わず色名だけ
//!     cargo run -- --keyboard                         マイクの代わりに手打ち
//!     cargo run --no-default-features                 whisper もウィンドウも無し
//!     DONNA_IRO_ASSETS=assets/reference cargo run     合成音で試す

mod audio;
mod color;
mod cue;
mod game;
mod listener;
mod matcher;
mod screen;
#[cfg(feature = "window")]
mod window;

use anyhow::Result;

use audio::Player;
use game::Game;
use listener::Listener;
use screen::Screen;

struct Options {
    keyboard: bool,
    terminal: bool,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let opts = Options {
        keyboard: args.iter().any(|a| a == "--keyboard"),
        terminal: args.iter().any(|a| a == "--terminal") || cfg!(not(feature = "window")),
    };

    // 遊んでいる最中に落ちないよう、素材は先に確かめる。
    audio::check_assets()?;

    if opts.terminal {
        return play(opts, Box::new(screen::Terminal));
    }

    #[cfg(feature = "window")]
    {
        // ウィンドウはメインスレッドに置く必要がある。しかも cpal も
        // rodio も !Send なので、ゲーム側をワーカースレッドに出して
        // そこで音の口を開く。
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            if let Err(e) = play(opts, Box::new(window::Remote(tx))) {
                eprintln!("エラー: {e:#}");
            }
            // ここで送信側が落ちるので、ウィンドウ側のループも抜ける。
        });
        // ウィンドウを閉じたら main が返り、プロセスごと終わる。
        window::run(rx)
    }
    #[cfg(not(feature = "window"))]
    unreachable!("terminal で分岐済み")
}

fn play(opts: Options, screen: Box<dyn Screen>) -> Result<()> {
    let player = Player::new()?;
    Game::new(player, open_ears(opts.keyboard)?, screen).run()
}

#[cfg(feature = "whisper")]
fn open_ears(by_keyboard: bool) -> Result<Box<dyn Listener>> {
    if by_keyboard {
        return Ok(Box::new(listener::Keyboard));
    }
    Ok(Box::new(listener::Mic::new(audio::Ears::new()?)?))
}

#[cfg(not(feature = "whisper"))]
fn open_ears(_by_keyboard: bool) -> Result<Box<dyn Listener>> {
    Ok(Box::new(listener::Keyboard))
}
