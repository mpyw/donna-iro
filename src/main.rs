//! 「どんな色がすき？」と呼びかけ、子どもが叫んだ色名を聞き取って
//! その色の続きを再生する。
//!
//!     cargo run                                       マイク
//!     cargo run --no-default-features                 キーボード（進行の確認用）
//!     DONNA_IRO_ASSETS=assets/reference cargo run     合成音で試す
//!     cargo run -- --keyboard                         マイクの代わりに手打ち
//!     cargo run -- --palette                          色味だけ見る

mod audio;
mod color;
mod cue;
mod display;
mod game;
mod listener;
mod matcher;

use anyhow::Result;

use audio::Player;
use color::Color;
use game::Game;
use listener::Listener;

fn main() -> Result<()> {
    if std::env::args().any(|a| a == "--palette") {
        return palette();
    }

    // 遊んでいる最中に落ちないよう、素材は先に確かめる。
    audio::check_assets()?;

    let by_keyboard = std::env::args().any(|a| a == "--keyboard");
    let player = Player::new()?;
    Game::new(player, open_ears(by_keyboard)?).run()
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

/// 色味の確認用。音源もマイクも要らない。
fn palette() -> Result<()> {
    let rgbs: Vec<_> = Color::ALL.iter().map(|c| c.rgb()).collect();
    print!("{}", display::all(&rgbs));
    for c in Color::ALL {
        println!("\n{c:?}");
        print!("{}", display::one(c.rgb()));
    }
    Ok(())
}
