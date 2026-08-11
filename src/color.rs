//! 認識対象の色。
//!
//! 語彙をここに閉じることで、汎用 ASR に頼らずとも判定できるようにする。
//!
//! **色ごとの情報は下の表に1行で書く。** 読み・漢字・RGB・表示名・素材名を
//! 別々の `match` に散らしていた頃は、色を足すたびに5箇所を回る必要があり、
//! しかも並びが揃っているかを目で確かめるしかなかった。表なら横に読める。

use strum::{EnumCount, VariantArray};

/// 光の三原色。色の言葉なのでここに置く。
///
/// 画面の側（`screen.rs`）に置いていたことがあり、そのせいで色が画面を
/// 読む向きになっていた。色は画面を知らなくても成り立つ。
pub type Rgb = (u8, u8, u8);

/// 色の定義から `Color` と、色ごとの値を返す `const fn` を一度に作る。
///
/// 項目を1つでも書き忘れれば、その色はパターンに合わずマクロ展開で止まる。
/// 生成される `match` は網羅的なままなので、変種を足して定義を書き忘れる
/// こともできない（そもそも変種はここからしか生えない）。
///
/// 項目の順番は下のパターンで固定してある。並べ替えたい理由が無いのと、
/// 揃っていないと読むときに目が滑るため。
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
            stem: $stem:literal,
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

            /// 音源のファイル名。`assets/<stem>.wav`。
            pub const fn stem(&self) -> &'static str {
                match self { $(Color::$variant => $stem,)* }
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
        stem: "red",
    },
    Blue: {
        reading: "あお",
        kanji: "青",
        rgb: (0, 104, 183),
        name: "青",
        stem: "blue",
    },
    Yellow: {
        reading: "きいろ",
        kanji: "黄色",
        rgb: (255, 241, 0),
        name: "黄",
        stem: "yellow",
    },
    Green: {
        reading: "みどり",
        kanji: "緑",
        rgb: (0, 153, 68),
        name: "緑",
        stem: "green",
    },
    YellowGreen: {
        reading: "きみどり",
        kanji: "黄緑",
        rgb: (143, 195, 31),
        name: "黄緑",
        stem: "yellowgreen",
    },
    White: {
        reading: "しろ",
        kanji: "白",
        rgb: (245, 245, 245),
        name: "白",
        stem: "white",
    },
    Black: {
        reading: "くろ",
        kanji: "黒",
        rgb: (35, 24, 21),
        name: "黒",
        stem: "black",
    },
    /// 「桃色」とは言われないので漢字表記は持たない。
    Pink: {
        reading: "ぴんく",
        rgb: (233, 84, 140),
        name: "ピンク",
        stem: "pink",
    },
    /// 「橙」は2歳児の語彙に無いので漢字表記は持たない。
    Orange: {
        reading: "おれんじ",
        rgb: (243, 152, 0),
        name: "オレンジ",
        stem: "orange",
    },
    Purple: {
        reading: "むらさき",
        kanji: "紫",
        rgb: (146, 7, 131),
        name: "紫",
        stem: "purple",
    },
    Brown: {
        reading: "ちゃいろ",
        kanji: "茶色",
        rgb: (122, 69, 26),
        name: "茶",
        stem: "brown",
    },
    LightBlue: {
        reading: "みずいろ",
        kanji: "水色",
        rgb: (0, 160, 233),
        name: "水色",
        stem: "lightblue",
    },
}

impl Color {
    /// 全色を宣言順に並べた配列。
    ///
    /// 一覧そのものは `VARIANTS` が持っている。ただしあちらはスライスなので、
    /// 並べ替えて持ち回る（フィナーレのシャッフル）には丈の決まった配列が要る。
    /// これはその入れ物への移し替えであって、一覧を二重に持つわけではない。
    ///
    /// `COUNT` と `VARIANTS` は同じ derive から出るので長さは必ず一致し、
    /// 添字が外れることはない。
    ///
    /// `std::array::from_fn` は const ではないので手で回している。const に
    /// しておくと定数として畳めて、`const` の初期値にも書ける。
    pub const fn all() -> [Color; Self::COUNT] {
        let mut out = [Color::VARIANTS[0]; Self::COUNT];
        let mut i = 1;
        while i < Self::COUNT {
            out[i] = Color::VARIANTS[i];
            i += 1;
        }
        out
    }

    pub fn random() -> Color {
        use rand::seq::SliceRandom;
        *Color::VARIANTS.choose(&mut rand::thread_rng()).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// const で受けられること自体が主張なので、const で受ける。
    /// **この関数はコンパイルが通った時点で半分終わっている。**
    #[test]
    fn is_usable_in_const_context() {
        const ALL: [Color; Color::COUNT] = Color::all();
        const RED: &str = Color::Red.reading();

        assert_eq!(ALL.as_slice(), Color::VARIANTS, "一覧が VARIANTS とずれた");
        assert_eq!(RED, "あか");
    }

    /// 素材名はファイル名になるので、日本語や大文字が混ざると事故る。
    /// 表に手で書く列なので、形だけ見ておく。
    #[test]
    fn stems_are_lowercase_ascii() {
        for c in Color::VARIANTS {
            let stem = c.stem();
            assert!(
                stem.chars().all(|ch| ch.is_ascii_lowercase()),
                "素材名が小文字の ASCII でない: {stem}"
            );
        }
    }
}
