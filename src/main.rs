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

mod app;
mod config;
mod io;

use anyhow::{Context, Result};

use app::{Control, Game, Listener, Screen};

struct Options {
    keyboard: bool,
    terminal: bool,
    once: bool,
    config: config::Config,
}

/// 引数を順に食べる。**知らないものは黙って捨てない。**
///
/// 拾い読みしていた頃は `--termnal` のようなタイポが黙ってウィンドウで
/// 起動し、`--config` の値が無ければ既定に落ちた。指定したのに効いて
/// いない、が一番たちが悪い。
fn parse_args() -> Result<Options> {
    let mut keyboard = false;
    let mut terminal = cfg!(not(feature = "window"));
    let mut once = false;
    let mut explicit: Option<std::path::PathBuf> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--keyboard" => keyboard = true,
            "--terminal" => terminal = true,
            "--once" => once = true,
            "--config" => {
                let path = args.next().context("--config にファイル名が無い")?;
                explicit = Some(std::path::PathBuf::from(path));
            }
            other => anyhow::bail!(
                "知らない引数: {other}\n  使えるのは --keyboard / --terminal / --once / --config <path>"
            ),
        }
    }

    Ok(Options {
        keyboard,
        terminal,
        once,
        config: config::load(explicit.as_deref())?,
    })
}

fn main() -> Result<()> {
    let opts = parse_args()?;

    // 在処を決めて、揃っているかまで見る。遊んでいる最中に落ちないように。
    io::audio::configure(&opts.config)?;

    if opts.terminal {
        let control: Box<dyn Control> = if opts.once {
            Box::new(io::control::Never)
        } else {
            Box::new(io::control::Stdin)
        };
        return play(opts, Box::new(io::screen::Terminal), control);
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
            Box::new(io::control::Never)
        } else {
            Box::new(io::control::Channel(again_rx))
        };
        // ゲーム側が落ちたことを main へ伝える口。**これが無いと、音源や
        // デバイスの障害で死んでも終了コードが0になる。** systemd の
        // 再起動条件や起動スクリプトからは成功に見えてしまう。
        let (dead_tx, dead_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = play(opts, Box::new(io::screen::Remote(tx)), control);
            if let Err(e) = &result {
                eprintln!("エラー: {e:#}");
            }
            let _ = dead_tx.send(result.is_err());
            // ここで送信側が落ちるので、ウィンドウ側のループも抜ける。
        });
        // ウィンドウを閉じたらプロセスごと終わる。
        let closed = io::screen::window::run(rx, again_tx);
        if let Err(e) = &closed {
            eprintln!("エラー: {e:#}");
        }
        // ゲーム側が先に落ちていれば、そちらの失敗を優先して報せる。
        let game_failed = dead_rx.try_recv().unwrap_or(false);
        quit(if closed.is_ok() && !game_failed { 0 } else { 1 })
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
    let player = Box::new(io::player::Speakers::new()?);
    let listener = open_listener(&opts)?;
    Game::new(player, listener, screen, control, &opts.config).run()
}

#[cfg(feature = "whisper")]
fn open_listener(opts: &Options) -> Result<Box<dyn Listener>> {
    if opts.keyboard {
        return Ok(Box::new(io::listener::Keyboard));
    }
    let ears = io::audio::Ears::new(&opts.config)?;
    Ok(Box::new(io::listener::Mic::new(ears, &opts.config)?))
}

#[cfg(not(feature = "whisper"))]
fn open_listener(_opts: &Options) -> Result<Box<dyn Listener>> {
    Ok(Box::new(io::listener::Keyboard))
}
