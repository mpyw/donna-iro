//! 「どんな色がすき？」と呼びかけ、子どもが叫んだ色名を聞き取って
//! その色の続きを再生する。
//!
//!     cargo run                             マイク＋ウィンドウ
//!     cargo run -- --terminal               ウィンドウを使わず色名だけ
//!     cargo run -- --keyboard               マイクの代わりに手打ち
//!     cargo run -- --config other.toml      別の設定ファイルを読む
//!     cargo run --no-default-features       whisper もウィンドウも無し
//!
//! 設定は `config.toml` に集約してある。何をいじれるかはそのファイルを見ること。

mod audio;
mod color;
mod config;
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
    config: config::Config,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let explicit = args
        .iter()
        .position(|a| a == "--config")
        .and_then(|i| args.get(i + 1))
        .map(std::path::PathBuf::from);
    let opts = Options {
        keyboard: args.iter().any(|a| a == "--keyboard"),
        terminal: args.iter().any(|a| a == "--terminal") || cfg!(not(feature = "window")),
        config: config::Config::load(explicit.as_deref())?,
    };

    // 遊んでいる最中に落ちないよう、素材は先に確かめる。
    audio::check_assets(&opts.config)?;

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
    let ears = open_ears(&opts)?;
    Game::new(player, ears, screen, &opts.config).run()
}

#[cfg(feature = "whisper")]
fn open_ears(opts: &Options) -> Result<Box<dyn Listener>> {
    if opts.keyboard {
        return Ok(Box::new(listener::Keyboard));
    }
    let ears = audio::Ears::new(&opts.config)?;
    Ok(Box::new(listener::Mic::new(ears, &opts.config)?))
}

#[cfg(not(feature = "whisper"))]
fn open_ears(_opts: &Options) -> Result<Box<dyn Listener>> {
    Ok(Box::new(listener::Keyboard))
}
