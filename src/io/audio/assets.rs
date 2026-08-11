//! 素材の在処。ファイルから読むか、バイナリに埋め込んだものを読むか。

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result};
#[cfg(feature = "embed-audio")]
use const_for::const_for;

use crate::app::cue::Cue;
use crate::config::Config;

/// ファイルから読むか、バイナリに埋め込んだものを読むか。
/// `rodio::Decoder` に渡すため、どちらも同じ型にまとめる。
pub enum Media {
    File(BufReader<File>),
    #[cfg(feature = "embed-audio")]
    Memory(std::io::Cursor<&'static [u8]>),
}

impl Media {
    pub fn open(cue: Cue) -> Result<Self> {
        let stem = cue.stem();

        // 埋め込みがあり、かつディレクトリ指定で上書きされていなければ
        // バイナリの中から鳴らす。
        #[cfg(feature = "embed-audio")]
        if asset_dir_override().is_none() {
            let bytes =
                embedded(stem).with_context(|| format!("埋め込まれていない素材: {stem}"))?;
            return Ok(Media::Memory(std::io::Cursor::new(bytes)));
        }

        let path = asset_path(stem);
        let file = File::open(&path).with_context(|| format!("音源がない: {}", path.display()))?;
        Ok(Media::File(BufReader::new(file)))
    }
}

impl Read for Media {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Media::File(f) => f.read(buf),
            #[cfg(feature = "embed-audio")]
            Media::Memory(c) => c.read(buf),
        }
    }
}

impl Seek for Media {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        match self {
            Media::File(f) => f.seek(pos),
            #[cfg(feature = "embed-audio")]
            Media::Memory(c) => c.seek(pos),
        }
    }
}

/// 音源のディレクトリ指定。設定で明示されていればそれを使う。
/// 埋め込みビルドでも、指定があればファイルから読む。
static OVERRIDE: OnceLock<Option<PathBuf>> = OnceLock::new();

fn asset_dir_override() -> Option<PathBuf> {
    OVERRIDE.get().cloned().flatten()
}

/// **起動時に一度だけ呼ぶ。** 素材の在処を決めて、揃っているかまで見る。
///
/// 在処を決めただけで先に進むと、足りないことに気づくのが遊んでいる最中に
/// なる。決めることと確かめることは離しても意味が無いので、ひとつにまとめる。
pub fn configure(cfg: &Config) -> Result<()> {
    let _ = OVERRIDE.set(cfg.paths.assets());
    check_assets()
}

fn asset_path(stem: &str) -> PathBuf {
    asset_dir().join(format!("{stem}.wav"))
}

/// 使う音源ディレクトリ。一度決めたら変えない。
///
/// 明示指定がなければ、本番音源が揃っているかを見て自動で選ぶ。
/// つくよみちゃんの音源を `assets/` に置いた時点で、何もしなくても
/// そちらに切り替わる。合成音を既定にベタ書きすると、本番音源を
/// 置いたあとも合成音が鳴り続けることになる。
fn asset_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        if let Some(dir) = asset_dir_override() {
            return dir;
        }
        let main = PathBuf::from("assets");
        if complete(&main) {
            return main;
        }
        let reference = main.join("reference");
        if complete(&reference) {
            eprintln!("  本番音源が無いので合成音を使う: {}", reference.display());
            return reference;
        }
        // どちらも欠けている。確認の側に本番側のパスで報告させる。
        main
    })
}

fn complete(dir: &Path) -> bool {
    Cue::every()
        .iter()
        .all(|c| dir.join(format!("{}.wav", c.stem())).exists())
}

/// 音源をバイナリに埋め込む。ラズパイに1ファイル置くだけで動かせる。
///
///     cargo build --release --features embed-audio
///
/// `include_bytes!` はコンパイル時にファイルを要求するので、
/// 音源が揃うまではこのフィーチャーを有効にできない。
///
/// モデルのほうは `embed-model` で埋め込む。両方入れると
/// バイナリ1つで完結する。
#[cfg(feature = "embed-audio")]
macro_rules! table {
    // 素材の一覧はここ1箇所。ディレクトリだけ差し替えられるようにしてある。
    ($dir:literal) => {
        table!(@ $dir,
            "intro", "question", "tail", "tail-lead", "bridge", "interlude", "all",
            "red", "blue", "yellow", "green", "yellowgreen", "white", "black",
            "pink", "orange", "purple", "brown", "lightblue",
        )
    };
    (@ $dir:literal, $($name:literal),* $(,)?) => {
        &[$(($name, include_bytes!(
            concat!(env!("CARGO_MANIFEST_DIR"), $dir, $name, ".wav")
        ) as &'static [u8])),*]
    };
}

/// 埋め込んだ実体。**名前は手で並べる。**
///
/// `include_bytes!` はリテラルのパスしか受け付けないので、`Cue::stem()` の
/// ように導くことができない。実体が無ければここで落ちるが、素材を足して
/// 表に書き忘れるほうは下の検査で見る。
///
/// 本番音源が無い状態で `.app` を作りたいときのために、`embed-reference` で
/// 確認用の合成音を埋め込める。**実行時のフォールバック（`asset_dir()` が
/// `assets/reference/` に落ちる）は埋め込みには効かない。** `include_bytes!`
/// はファイルの有無を見られないので、こちらは手で切り替えるしかない。
#[cfg(all(feature = "embed-audio", not(feature = "embed-reference")))]
const FILES: &[(&str, &[u8])] = table!("/assets/");
#[cfg(feature = "embed-reference")]
const FILES: &[(&str, &[u8])] = table!("/assets/reference/");

/// const で使う文字列の等値。`==` は const で回せない。
#[cfg(feature = "embed-audio")]
const fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    const_for!(i in 0..a.len() => {
        if a[i] != b[i] {
            return false;
        }
    });
    true
}

/// **素材がすべて表に載っているか、コンパイル時に確かめる。**
///
/// 実体が無ければ `include_bytes!` がその場で落ちるので、ここで見るのは
/// 名前の取りこぼしだけ。色を足して上の表に書き忘れると、以前は鳴らそう
/// とした時点で初めて「埋め込まれていない素材」になっていた。
#[cfg(feature = "embed-audio")]
const _: () = {
    let every = Cue::every();
    const_for!(i in 0..Cue::COUNT => {
        let mut found = false;
        const_for!(j in 0..FILES.len() => {
            if str_eq(FILES[j].0, every[i].stem()) {
                found = true;
            }
        });
        assert!(found, "埋め込みの表に無い素材がある（audio.rs の FILES を見ること）");
    });
};

#[cfg(feature = "embed-audio")]
fn embedded(stem: &str) -> Option<&'static [u8]> {
    FILES.iter().find(|(n, _)| *n == stem).map(|(_, b)| *b)
}

/// 素材が揃っているか確かめる。足りないものはまとめて出す。
///
/// 在処が決まっていることが前提なので、`configure` からしか呼ばない。
fn check_assets() -> Result<()> {
    let missing: Vec<&str> = Cue::every()
        .into_iter()
        .filter(|&c| Media::open(c).is_err())
        .map(|c| c.stem())
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    anyhow::bail!(
        "音源が足りない（{} / {}）: {}",
        missing.len(),
        Cue::every().len(),
        missing.join(", ")
    )
}
