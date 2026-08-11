//! 外の世界に触るもの。`app` のトレイトの実装が並ぶ。
//!
//! スピーカー、マイク、画面、標準入出力、設定ファイル。どれも
//! `app` 側のトレイトを実装するか、`app` の型を組み立てて返すだけで、
//! 遊びの進行は知らない。
//!
//! **`io` から `app` を読むのは構わない。** 逆はいけない。

pub mod audio;
pub mod config;
pub mod control;
pub mod listener;
pub mod player;
pub mod screen;
