//! 「どんな色がすき？」と呼びかけ、子どもが叫んだ色名を聞き取って
//! その色の続きを再生する。
//!
//!     cargo run                             マイク＋ウィンドウ
//!     cargo run -- --terminal               ウィンドウを使わず色名だけ
//!     cargo run -- --keyboard               マイクの代わりに手打ち
//!     cargo run -- --once                   フィナーレで終わる（もう1回を待たない）
//!     cargo run -- --config other.toml      別の設定ファイルを読む
//!     cargo run --no-default-features       whisper もウィンドウも無し
//!
//! 設定は `config.toml` に集約してある。何をいじれるかはそのファイルを見ること。

mod audio;
mod color;
mod config;
mod control;
mod cue;
mod game;
mod listener;
mod matcher;
mod screen;

use anyhow::Result;

use audio::Player;
use control::Control;
use game::Game;
use listener::Listener;
use screen::Screen;

struct Options {
    keyboard: bool,
    terminal: bool,
    once: bool,
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
        once: args.iter().any(|a| a == "--once"),
        config: config::Config::load(explicit.as_deref())?,
    };

    // 遊んでいる最中に落ちないよう、素材は先に確かめる。
    audio::check_assets(&opts.config)?;

    if opts.terminal {
        let control: Box<dyn Control> = if opts.once {
            Box::new(control::never::Never)
        } else {
            Box::new(control::stdin::Stdin)
        };
        return play(opts, Box::new(screen::terminal::Terminal), control);
    }

    #[cfg(feature = "window")]
    {
        // ウィンドウはメインスレッドに置く必要がある。しかも cpal も
        // rodio も !Send なので、ゲーム側をワーカースレッドに出して
        // そこで音の口を開く。
        let (tx, rx) = std::sync::mpsc::channel();
        // 逆向きに1本。フレームを送るのと向きだけが違う。
        let (again_tx, again_rx) = std::sync::mpsc::channel();
        let control: Box<dyn Control> = if opts.once {
            drop(again_rx);
            Box::new(control::never::Never)
        } else {
            Box::new(control::channel::Channel(again_rx))
        };
        std::thread::spawn(move || {
            if let Err(e) = play(opts, Box::new(screen::window::Remote(tx)), control) {
                eprintln!("エラー: {e:#}");
            }
            // ここで送信側が落ちるので、ウィンドウ側のループも抜ける。
        });
        // ウィンドウを閉じたらプロセスごと終わる。
        let closed = screen::window::run(rx, again_tx);
        if let Err(e) = &closed {
            eprintln!("エラー: {e:#}");
        }
        quit(if closed.is_ok() { 0 } else { 1 })
    }
    #[cfg(not(feature = "window"))]
    unreachable!("terminal で分岐済み")
}

/// 後片付けをせずにプロセスを終わらせる。
///
/// ウィンドウを閉じた時点でも、ゲーム側のスレッドは音を鳴らしていたり
/// whisper で認識している最中だったりする。止める手立ては無いので、
/// そのまま main を返す（= `exit(3)` を通る）ことになるが、これだと
/// whisper.cpp の Metal バックエンドが atexit で GPU デバイスを畳む際に
/// 「まだ誰かがバッファを握っている」と気づいて落ちる。
///
///     ggml-metal-device.m: GGML_ASSERT([rsets->data count] == 0) failed
///
/// 終わるだけなので解放し損ねても困らない。`_exit(2)` で C++ の静的
/// デストラクタを走らせずに抜ける。バッファリングされた出力は
/// 捨てられてしまうので、先に流しておく。
#[cfg(feature = "window")]
fn quit(code: i32) -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    extern "C" {
        fn _exit(code: i32) -> !;
    }
    unsafe { _exit(code) }
}

fn play(opts: Options, screen: Box<dyn Screen>, control: Box<dyn Control>) -> Result<()> {
    let player = Player::new()?;
    let ears = open_ears(&opts)?;
    Game::new(player, ears, screen, control, &opts.config).run()
}

#[cfg(feature = "whisper")]
fn open_ears(opts: &Options) -> Result<Box<dyn Listener>> {
    if opts.keyboard {
        return Ok(Box::new(listener::keyboard::Keyboard));
    }
    let ears = audio::Ears::new(&opts.config)?;
    Ok(Box::new(listener::mic::Mic::new(ears, &opts.config)?))
}

#[cfg(not(feature = "whisper"))]
fn open_ears(_opts: &Options) -> Result<Box<dyn Listener>> {
    Ok(Box::new(listener::keyboard::Keyboard))
}
