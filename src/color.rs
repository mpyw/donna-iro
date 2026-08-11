//! 認識対象の色と、子どもの応答。
//!
//! **語彙をここに閉じることで、汎用 ASR に頼らずとも判定できるようにする。**
//! 「ぜんぶ」だけ `matcher.rs` に定数で置いていた頃は、この方針から漏れて
//! いて、語彙を使う側（判定と `initial_prompt`）の両方に手で足していた。
//! 色と同じ `Answer` に収めたので、どちらも同じ一覧を見るだけで済む。
//!
//! **色ごとの情報は下の定義に1箇所でまとめて書く。** 読み・漢字・RGB・表示名を
//! 別々の `match` に散らしていた頃は、色を足すたびに5箇所を回る必要があり、
//! 対応が合っているかを目で数えるしかなかった。
//!
//! 素材名は書かない。変種名を小文字にしたものが既定になる。

use strum::{EnumCount, VariantArray};

use const_for::const_for;

/// 光の三原色。色の言葉なのでここに置く。
///
/// 画面の側（`screen.rs`）に置いていたことがあり、そのせいで色が画面を
/// 読む向きになっていた。色は画面を知らなくても成り立つ。
pub type Rgb = (u8, u8, u8);

/// ASCII を小文字にする。変種名から素材名を作るためだけのもの。
///
/// `const` で回すには長さが型に載っている必要があるので、呼ぶ側が
/// `stringify!` の長さを渡す。非 ASCII は来ない（変種名は識別子）。
const fn lower<const N: usize>(s: &str) -> [u8; N] {
    let bytes = s.as_bytes();
    let mut out = [0u8; N];
    const_for!(i in 0..N => {
        out[i] = bytes[i].to_ascii_lowercase();
    });
    out
}

/// 色の定義から `Color` と、色ごとの値を返す `const fn` を一度に作る。
///
/// 項目を1つでも書き忘れれば、その色はパターンに合わずマクロ展開で止まる。
/// 生成される `match` は網羅的なままなので、変種を足して定義を書き忘れる
/// こともできない（そもそも変種はここからしか生えない）。
///
/// 項目の順番は下のパターンで固定してある。並べ替えたい理由が無いのと、
/// 揃っていないと読むときに目が滑るため。
///
/// **`reading` と `kanji` は正規形で書くこと。** 判定側（`matcher.rs`）は
/// 語彙をそのまま突き合わせる。カタカナや空白が混ざると、そこだけ黙って
/// 当たらなくなる。読みはひらがな、空白なし。カタカナ表記を並べる必要は
/// ない（認識結果の側がひらがなに寄せられる）。
macro_rules! colors {
    // `kanji` の行が無ければ `None`。漢字表記を持たない色があるので、
    // 全部の色に `Some(..)` / `None` を書かせるより「無い色は書かない」ほうが
    // 表として素直に読める。
    (@kanji) => { None };
    (@kanji $kanji:literal) => { Some($kanji) };

    ($(
        $(#[$meta:meta])*
        $variant:ident: {
            reading: $reading:literal,
            $(kanji: $kanji:literal,)?
            rgb: $rgb:expr,
            name: $name:literal,
        },
    )*) => {
        /// 歌がクレヨンの歌なので、標準的なクレヨン12色セット
        /// （しろ・きいろ・きみどり・みどり・みずいろ・あお・むらさき・
        /// ももいろ・あか・だいだい・ちゃいろ・くろ）に揃えてある。
        ///
        /// 並び順は画面に出す順（4列×3行）でもある。`VARIANTS` は宣言順に
        /// 返るので、並べ替えたいときは表の行を入れ替えること。
        #[derive(Debug, Clone, Copy, PartialEq, Eq, EnumCount, VariantArray)]
        pub enum Color {
            $($(#[$meta])* $variant,)*
        }

        impl Color {
            /// 全色。表に並べた順そのまま。
            ///
            /// `VARIANTS` と中身は同じだが、あちらはスライス。並べ替えて
            /// 持ち回る（フィナーレのシャッフル）には丈の決まった配列が要る。
            /// 一覧を二重に持っているわけではなく、どちらも表から生える。
            pub const ALL: [Color; Self::COUNT] = [$(Color::$variant,)*];

            /// 認識結果にマッチさせる読み。ひらがなの正式形をひとつだけ持つ。
            ///
            /// 表記ゆれや聞き間違いを並べていた時期もあったが、
            /// `initial_prompt` でこの語彙そのものを whisper に教えるように
            /// したので不要になった。残りのゆれは編集距離で吸収する。
            pub const fn reading(&self) -> &'static str {
                match self { $(Color::$variant => $reading,)* }
            }

            /// 漢字表記。`initial_prompt` は誘導であって強制ではないので、
            /// whisper は漢字を返すことがある。ひらがなに機械変換できないため、
            /// 判定用に一形だけ持っておく。カタカナ表記の色は正規化で
            /// ひらがなに寄るので不要。
            pub const fn kanji(&self) -> Option<&'static str> {
                match self { $(Color::$variant => colors!(@kanji $($kanji)?),)* }
            }

            /// 画面に描く●の色。クレヨン12色の実際の色味に寄せてある。
            pub const fn rgb(&self) -> Rgb {
                match self { $(Color::$variant => $rgb,)* }
            }

            /// ターミナルに出す名前。動作確認用なので読めれば良い。
            pub const fn name(&self) -> &'static str {
                match self { $(Color::$variant => $name,)* }
            }

            /// 音源のファイル名。`assets/<stem>.wav`。変種名の小文字。
            ///
            /// `macro_rules!` に大文字小文字の変換は無いので `const fn` で回す。
            /// 長さを型に載せないと配列が作れないため `stringify!` の長さを渡す。
            pub const fn stem(&self) -> &'static str {
                match self {
                    $(Color::$variant => {
                        const N: usize = stringify!($variant).len();
                        static BYTES: [u8; N] = lower::<N>(stringify!($variant));
                        match std::str::from_utf8(&BYTES) {
                            Ok(s) => s,
                            Err(_) => panic!("変種名は ASCII のはず"),
                        }
                    })*
                }
            }
        }
    };
}

colors! {
    Red: {
        reading: "あか",
        kanji: "赤",
        rgb: (230, 0, 18),
        name: "赤",
    },
    Blue: {
        reading: "あお",
        kanji: "青",
        rgb: (0, 104, 183),
        name: "青",
    },
    Yellow: {
        reading: "きいろ",
        kanji: "黄色",
        rgb: (255, 241, 0),
        name: "黄",
    },
    Green: {
        reading: "みどり",
        kanji: "緑",
        rgb: (0, 153, 68),
        name: "緑",
    },
    YellowGreen: {
        reading: "きみどり",
        kanji: "黄緑",
        rgb: (143, 195, 31),
        name: "黄緑",
    },
    White: {
        reading: "しろ",
        kanji: "白",
        rgb: (245, 245, 245),
        name: "白",
    },
    Black: {
        reading: "くろ",
        kanji: "黒",
        rgb: (35, 24, 21),
        name: "黒",
    },
    /// 「桃色」とは言われないので漢字表記は持たない。
    Pink: {
        reading: "ぴんく",
        rgb: (233, 84, 140),
        name: "ピンク",
    },
    /// 「橙」は2歳児の語彙に無いので漢字表記は持たない。
    Orange: {
        reading: "おれんじ",
        rgb: (243, 152, 0),
        name: "オレンジ",
    },
    Purple: {
        reading: "むらさき",
        kanji: "紫",
        rgb: (146, 7, 131),
        name: "紫",
    },
    Brown: {
        reading: "ちゃいろ",
        kanji: "茶色",
        rgb: (122, 69, 26),
        name: "茶",
    },
    LightBlue: {
        reading: "みずいろ",
        kanji: "水色",
        rgb: (0, 160, 233),
        name: "水色",
    },
}

impl Color {
    pub fn random() -> Color {
        use rand::seq::SliceRandom;
        *Color::VARIANTS.choose(&mut rand::thread_rng()).unwrap()
    }
}

/// 子どもの応答。**この遊びの語彙そのもの。**
///
/// 判定の候補も `initial_prompt` に渡す語彙も、どちらもここから作る。
/// 「ぜんぶ」を色とは別の定数として持っていた頃は、増やすときに
/// `Matcher` と `Listener` の両方へ手で足す必要があった。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// 色を答えた。ループを続ける。
    Single(Color),
    /// 「ぜんぶ！」と答えた。転調してエンディングへ向かう。
    All,
}

impl Answer {
    pub const COUNT: usize = Color::COUNT + 1;

    /// 語彙の全体。色のうしろに「ぜんぶ」がひとつ。
    ///
    /// 並びは候補の優先順位に効く（同じ長さなら先に入れたほうが勝つ）。
    /// 「ぜんぶ」を末尾に置いてあるのは、同じ3文字の色名を押しのけない
    /// ようにするため。
    pub const fn every() -> [Answer; Self::COUNT] {
        let mut out = [Answer::All; Self::COUNT];
        const_for!(i in 0..Color::COUNT => {
            out[i] = Answer::Single(Color::VARIANTS[i]);
        });
        out
    }

    /// 認識結果にマッチさせる読み。ひらがなの正式形をひとつだけ。
    ///
    /// 色と同じく**正規形で書くこと**（ひらがな・空白なし）。判定側は
    /// これをそのまま突き合わせる。
    pub const fn reading(&self) -> &'static str {
        match self {
            Answer::Single(c) => c.reading(),
            Answer::All => "ぜんぶ",
        }
    }

    /// 漢字表記。`initial_prompt` は誘導であって強制ではないので、
    /// whisper は漢字を返すことがある。実際に「全部」が出た。
    pub const fn kanji(&self) -> Option<&'static str> {
        match self {
            Answer::Single(c) => c.kanji(),
            Answer::All => Some("全部"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// const で受けられること自体が主張なので、const で受ける。
    /// **この関数はコンパイルが通った時点で半分終わっている。**
    #[test]
    fn is_usable_in_const_context() {
        const RED: &str = Color::Red.reading();

        assert_eq!(
            Color::ALL.as_slice(),
            Color::VARIANTS,
            "ALL と VARIANTS がずれた"
        );
        assert_eq!(RED, "あか");
    }

    /// 素材名は変種名を `lower` で小文字にして作る。ファイル名になるので、
    /// 大文字や区切りが混ざると `assets/<stem>.wav` が見つからなくなる。
    /// `lower` が期待どおり動いていることをここで押さえる。
    #[test]
    fn stems_are_the_variant_name_lowercased() {
        for c in Color::VARIANTS {
            let expected = format!("{c:?}").to_ascii_lowercase();
            assert_eq!(c.stem(), expected, "素材名が変種名の小文字になっていない");
        }
        // 語の区切りは入らない（YellowGreen → yellowgreen）。
        assert_eq!(Color::YellowGreen.stem(), "yellowgreen");
    }
}
