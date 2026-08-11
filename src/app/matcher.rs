//! 認識結果の文字列から応答を判定する。
//!
//! **信用できるのは先頭だけ。** tiny は短い音を渡されるとデコーダが
//! 暴走して、言っていない語を後ろに継ぎ足すことがある。しかも同じ語の
//! 繰り返しとは限らず、別の色が混ざる。だから後ろは捨てる。
//!
//! **位置が最優先。** 先頭の区間から順に見て、当たった時点で決める。
//!
//! 段の優先度を位置より上に置くと、後ろの区間が完全一致しただけで
//! 先頭を追い越してしまう。「みじろ、きいろ」で実際にそうなった。
//! 先頭は音の近さでしか当たらないが、それでも先頭を採るべき。
//!
//! **認識は語を割る**（「ちゃいろ」が「じゃあ、いろ」になった）ので、
//! 単独と、次の区間と繋げたものを織り交ぜて見る。位置ごとの順は
//!
//! 1. 単独の完全一致
//! 2. 単独の部分一致（長い読みから順に）
//! 3. 結合の完全一致
//! 4. 結合の部分一致のうち、**境目をまたぐもの**
//! 5. 音の近さ。単独と結合を突き合わせて**距離の近いほう**
//! 6. 単独がどれにも当たらなければ、結合の部分一致と音の近さ
//!
//! 3〜5 の位置が要点で、詳しくは `find` の中に書いてある。順番を素直に
//! 「単独を3段 → 結合を3段」にすると、割れた語の前半がたまたま別の色に
//! 近いだけで確定してしまう。
//!
//! どれも該当しなければ `None`。呼び出し側でランダムな色に倒す。

use sort_const::const_quicksort;

use crate::app::color::Answer;
use const_for::const_for;

/// 質問の歌がマイクに回り込んだぶんを落とす。
///
/// 「どんないろがすき」には「いろ」が含まれるので、残したまま判定すると
/// きいろの読み「いーろ」に化けることがある。長いものから順に削る。
const PROMPT: &[&str] = &["どんないろがすき", "どんないろ", "どんな"];

/// 質問文そのもの。**区間まるごとがこの一部なら、答えではなく回り込み。**
///
/// `PROMPT` を削るだけでは足りない。whisper が「どんな いろ」と空白で
/// 割って返すと、区切ってから削るので破片の「いろ」が生き残り、音の
/// 近さで「しろ」に当たる。**しかも位置を最優先するので、後ろに続く
/// 本当の答えを差し置いて毎回それが鳴る。**
///
/// 色の読みはどれもこの文の一部ではないので、丸ごと一致で捨ててよい。
/// 破れていないことは `no_answer_hides_inside_the_question` で見る。
const QUESTION: &str = "どんないろがすき";

/// 拗音と長音を落とした骨格。編集距離を測るときだけ使う。
///
/// 幼児の発音は「ぜ」が「じぇ」に、「ぶ」が「ば」に寄る。小書き文字と
/// 長音符が入ると字数がずれて、素の編集距離では届かなくなる。
/// 落としてから比べると、残るのは子音・母音のずれだけになる。
///
/// 読みの側も同じ関数を通すので、「ちゃいろ」が「ちいろ」に潰れても
/// 両側が揃っていて問題ない。
fn skeleton(s: &str) -> Vec<char> {
    s.chars()
        .filter(|c| {
            !matches!(
                c,
                'ー' | 'ぁ' | 'ぃ' | 'ぅ' | 'ぇ' | 'ぉ' | 'ゃ' | 'ゅ' | 'ょ' | 'ゎ' | 'っ'
            )
        })
        .collect()
}

/// 漢字表記を持つ応答の数。候補の丈を出すためだけに数える。
const fn kanji_count() -> usize {
    let every = Answer::every();
    let mut n = 0;
    const_for!(i in 0..Answer::COUNT => {
        if every[i].kanji().is_some() {
            n += 1;
        }
    });
    n
}

/// UTF-8 の文字数。`str::chars` は const で回せないので先頭バイトを数える。
const fn char_count(s: &str) -> usize {
    let bytes = s.as_bytes();
    let mut n = 0;
    const_for!(i in 0..bytes.len() => {
        // 継続バイト（10xxxxxx）以外が1文字の先頭。
        if bytes[i] & 0xC0 != 0x80 {
            n += 1;
        }
    });
    n
}

/// 候補の数。読みが全応答ぶん、漢字はある応答のぶんだけ。
const CANDIDATE_COUNT: usize = Answer::COUNT + kanji_count();

/// 判定の候補。
///
/// **`normalize` は通さない。** 語彙の側が正規形で書かれている約束
/// （`color.rs` を見ること）なので、通しても何も起きない。破れていない
/// ことは `vocabulary_is_already_normalized` で見ている。
const CANDIDATES: [(Answer, &str); CANDIDATE_COUNT] = {
    // 候補の並び順。**同点が出ないところまで見る。**
    //
    // 長さの降順が主。短い色名が長い色名に含まれる組（「きみどり」⊃「みどり」）が
    // あるので、部分一致は長いほうから試さないと取りこぼす。
    //
    // 同じ長さなら元の位置の昇順。作る順は「読みを全部 → 漢字を全部」で、
    // どちらも `Answer::every()` の順なので、これで「読みが漢字より先」と
    // 「先に宣言した色が先」の両方が決まる。
    //
    // **ここまで見れば同点が無い。** 並べ替えが安定かどうかに結果が依らない
    // ので、不安定なアルゴリズムを使ってもよい。
    const fn before(a: (Answer, &str, usize), b: (Answer, &str, usize)) -> bool {
        let (la, lb) = (char_count(a.1), char_count(b.1));
        if la != lb {
            la > lb
        } else {
            a.2 < b.2
        }
    }

    let every = Answer::every();

    // 3つ目は作った順。並べ替えの second key に使う。
    let mut buf = [(Answer::All, "", 0usize); CANDIDATE_COUNT];
    let mut n = 0;
    const_for!(i in 0..Answer::COUNT => {
        buf[n] = (every[i], every[i].reading(), n);
        n += 1;
    });
    const_for!(i in 0..Answer::COUNT => {
        if let Some(k) = every[i].kanji() {
            buf[n] = (every[i], k, n);
            n += 1;
        }
    });

    // 配列を渡すと**その場では並べ替えず、並べ替えたものを返す**。
    // 戻り値を捨てると黙って未整列のまま進むので、必ず受け直すこと。
    let buf = const_quicksort!(buf, |a, b| before(*a, *b));

    // 並べ終わったら位置は要らない。
    let mut out = [(Answer::All, ""); CANDIDATE_COUNT];
    const_for!(i in 0..CANDIDATE_COUNT => {
        out[i] = (buf[i].0, buf[i].1);
    });
    out
};

pub struct Matcher {
    /// 候補の骨格。編集距離用。並びは `CANDIDATES` と同じ。
    ///
    /// `skeleton` が `Vec<char>` を返すので、ここだけは実行時に作る。
    skeletons: Vec<(Answer, Vec<char>)>,
    /// 先頭から信用する区間の数。tiny は言っていない語を後ろに継ぎ足す。
    head: usize,
}

impl Matcher {
    pub fn new(head: usize) -> Self {
        Self {
            // **骨格は読みだけ。** 漢字は `decompose` できず全文字が
            // 満額の置換になるので、音の近さの土俵に乗らない。無駄な
            // 距離計算になるうえ、同点（→ 諦める）の芽にもなる。
            // 漢字は完全一致と部分一致の2段で拾えている。
            skeletons: CANDIDATES
                .iter()
                .filter(|(a, r)| a.kanji() != Some(*r))
                .map(|(a, r)| (*a, skeleton(r)))
                .collect(),
            head: head.max(1),
        }
    }

    pub fn find(&self, raw: &str) -> Option<Answer> {
        let parts = assemble(raw, self.head);

        // 先頭の区間から順に。単独で当たらなければ次と繋げて試す。
        //
        // **結合の完全一致は、単独の音の近さより先に見る。** 単独で3段を
        // 使い切ってから結合に移ると、割れた語の前半がたまたま別の色に
        // 近いだけで確定してしまう。
        //
        //     「むら さき」→ くろ    （むら が d=4 で当たる）
        //     「きみ どり」→ きいろ  （きみ が d=6 で当たる）
        //
        // 空白が区切りになる前は `normalize` が繋ぎ直していたので、この段の
        // 脆さは読点割れでしか出なかった。
        (0..parts.len()).find_map(|i| {
            let single = parts[i].as_str();
            let joined = (parts.len() - i >= 2).then(|| parts[i..i + 2].concat());
            let joined = joined.as_deref();

            if let Some(a) = self.exact(single).or_else(|| self.substring(single)) {
                return Some(a);
            }
            if let Some(a) = joined.and_then(|j| self.exact(j)) {
                return Some(a);
            }
            // **境目をまたぐ部分一致は、単独の音の近さより先。** またぐ
            // ということは、そこで割れたのが誤りだったという証拠になる。
            //
            //     「むら さきいろ」 むらさき が境目をまたぐ  → むらさき
            //     「みじろ、きいろ」 きいろ は後ろの区間に収まる → 無視
            //
            // 後者を採ると幻覚の継ぎ足しを拾ってしまうので、またぐものだけ。
            if let Some(a) = joined.and_then(|j| straddling(j, single.len())) {
                return Some(a);
            }

            // ここから先は音の近さ。**単独が土俵に乗っているかで分かれる。**
            match self.nearest(single) {
                // 乗っている。結合と突き合わせて近いほうを採る。距離
                // そのものが確からしさなので、順番で決めるところではない。
                //
                //     「むら さぎ」    むら → くろ d=4 / むらさぎ → むらさき d=1
                //     「みじろ、きいろ」 みじろ → みずいろ d=4 / 結合 d=10
                //
                // 前者は結合、後者は単独。順番を固定すると必ずどちらかを
                // 外す。同点なら単独。位置を優先する方針に揃える。
                //
                // ここで結合の**部分一致**に譲らないのが要点。譲ると
                // 「みじろ、きいろ」が きいろ になり、幻覚の継ぎ足しを
                // 捨てる仕掛けが壊れる。
                Some((sa, sd)) => Some(match joined.and_then(|j| self.nearest(j)) {
                    Some((ja, jd)) if jd < sd => ja,
                    _ => sa,
                }),
                // 乗っていない。**部分一致のほうが音の近さより確か。**
                // 「じゃあ どんな いろ が すき みどり」の「じゃあ」は
                // どの色にも近くないが、「じゃあみどり」は きみどり に
                // 音だけなら近い。部分一致に先を譲れば みどり が残る。
                None => joined.and_then(|j| self.substring(j)).or_else(|| {
                    // 結合の音の近さは最後の手段。**次の区間が単独で
                    // もっと確かに当たるなら、そちらに譲る。**
                    //
                    //     「えー ちろ」  えーちろ → ちゃいろ / ちろ → しろ d=1
                    //
                    // 位置は最優先だが、先頭がどの段にも乗らなかった以上、
                    // その位置に主張は無い。譲って次の位置に任せる。
                    // 割れた語（「じゃん ぶー」）は結合のほうが近いので残る。
                    //
                    // **譲るのは負けたときだけ。** 同点で譲ると、同じ
                    // 発話が区間の割れ方だけで別の色になる。
                    //
                    // ```text
                    // みずろ   => みずいろ
                    // み ずろ  => くろ      （結合と次の区間が同点）
                    // ```
                    let (a, d) = joined.and_then(|j| self.nearest(j))?;
                    match parts.get(i + 1).and_then(|n| self.nearest(n)) {
                        Some((_, next_d)) if next_d < d => None,
                        _ => Some(a),
                    }
                }),
            }
        })
    }

    fn exact(&self, text: &str) -> Option<Answer> {
        CANDIDATES.iter().find(|(_, r)| *r == text).map(|(a, _)| *a)
    }

    fn substring(&self, text: &str) -> Option<Answer> {
        CANDIDATES
            .iter()
            .find(|(_, r)| text.contains(*r))
            .map(|(a, _)| *a)
    }

    fn nearest(&self, text: &str) -> Option<(Answer, usize)> {
        // 許容する編集距離。短い読みほど厳しくする。
        //
        // 単位は `substitution` の重みなので、4 が「1音ぶんのずれ」にあたる。
        //
        // **2文字の読みは半音まで。** 2音のうち1音が別物なら、それは
        // 「近い」ではなく「半分違う」。ここを1音まで許していた頃は、
        // 2モーラのフィラーが片端から色になっていた。
        //
        //     あの → あお d=3 / この → くろ d=4 / その → しろ d=4
        //
        // **「あの」は2歳児の言い出しの定番。** 位置が最優先なので
        // 「あの、あか」は毎回きまって あお になる。骨格1文字を捨てた
        // のと同じ話が、2モーラでも起きていた。
        //
        // 3文字以上は1音半まで許す。
        //
        // **「ぜんぶ」だけは1音までに絞る。** 誤検出するとゲームが終わって
        // しまうため。緩めると「でんわ」あたりが引っかかる。
        // 逆に取りこぼしても次の周回でまた聞けるので、そちらの害は小さい。
        const fn allowed(answer: Answer, skeleton_len: usize) -> usize {
            match answer {
                Answer::All => 4,
                Answer::Single(_) if skeleton_len <= 2 => 2,
                Answer::Single(_) => 6,
            }
        }

        let chars = skeleton(text);
        // **1音では土俵に乗らない。** どの読みも骨格で2文字以上あるので、
        // 1文字が当たったとしてもそれは「近い」のではなく「短すぎて何にでも
        // 近い」。「えー」が「あお」に化けて、位置優先で後ろの本当の答えを
        // 押し出していた。
        if chars.len() < 2 {
            return None;
        }
        let mut scores: Vec<(Answer, usize)> = Vec::new();
        for (a, r) in self.skeletons.iter() {
            let d = levenshtein(&chars, r);
            // **短いほうで測る。** 入力が2音しかないのに3音の読みへ
            // 1音半まで許すと、足りないぶんの挿入がただになる。
            // 「この」が きいろ に当たっていた。
            //
            // ただし**入力が候補の頭そのものなら、言い切っただけ**なので
            // 元の丈で測る。「みず」（みずいろ）「みど」（みどり）は
            // 2歳児が普通に言う。「この」「あの」はどの読みの頭でも
            // ないので、フィラーの誤射は戻らない。
            let truncated = chars.len() >= 2 && r.starts_with(chars.as_slice());
            let len = if truncated {
                r.len()
            } else {
                r.len().min(chars.len())
            };
            if d > allowed(*a, len) {
                continue;
            }
            scores.push((*a, d));
        }
        scores.sort_by_key(|&(_, d)| d);

        let (best, best_d) = *scores.first()?;
        // 同点なら諦める。「あか」と「あお」、「しろ」と「くろ」のように
        // 1文字違いの色があるので、割れたまま採用すると取り違える。
        if scores.get(1).is_some_and(|&(_, d)| d == best_d) {
            return None;
        }
        Some((best, best_d))
    }
}

/// 認識結果を、判定にかける区間の列にする。
///
/// **判定と分けてあるのは、ここが状態機械だから。** 溜め・繋ぎ先・質問の
/// 破片という3つの規則が絡み、直近の穴はどれも個別の入力ではなく規則同士の
/// 組み合わせで出た。`assembles_the_segments` で、出来上がった列そのものを
/// 見られるようにしてある。
///
/// `head` は先頭から信用する区間の数。tiny は言っていない語を後ろに継ぎ足す。
fn assemble(raw: &str, head: usize) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    // 直前の区間へ繋いでよいか。**質問が挟まった時点で切れる。**
    let mut joinable = false;
    // まだ実のある区間が1つも無いときの溜め。**破片は後ろ向きにしか
    // 繋げないので、割れた語の1つ目が質問文の一部だと繋ぎ先が無い。**
    // 「き、いろ」の「き」は「すき」の一部なので、そのまま捨てると
    // きいろ が丸ごと消える。
    let mut pending: Vec<String> = Vec::new();

    for seg in segments(&normalize(raw)) {
        if seg.is_empty() {
            // 区切りが続いただけ。何も起きていない。
            continue;
        }
        let seg = strip_prompt(&seg);
        if seg.is_empty() {
            // まるごと質問だった。**ここから後ろの破片は、前の語の
            // 続きではなく質問の続き。** 繋ぎ先を切り、溜めも捨てる。
            joinable = false;
            pending.clear();
            continue;
        }
        if QUESTION.contains(seg.as_str()) {
            // 質問文の破片。**単独では答えにならない。** ただし認識が
            // 語を割っただけということもある（「ちゃいろ」→「じゃあ、
            // いろ」）ので、直前が実のある区間ならそちらへ繋ぐ。
            //
            // 繋ぎ先が切れていれば捨てる。切らずにいると
            // 「じゃあ どんな いろ あか」の「いろ」が「じゃあ」に
            // 付いて「ちゃいろ」になり、また「あか」を押し出す。
            //
            // **捨てるのは `truncate` の前でなければならない。** 破片が
            // 席を占めると、後ろの本当の答えが `head` から押し出される。
            if parts.is_empty() {
                // 繋ぎ先がまだ無い。溜めておいて、**質問文の一部で
                // なくなった時点で**見極める。
                //
                // 質問がそのまま割れて届くぶんには、順番どおり
                // 「いろ」「いろが」「いろがすき」と伸びるので
                // 一度も抜けない。
                pending.push(seg);
                if !QUESTION.contains(pending.concat().as_str()) {
                    // **抜けたぶんを丸ごと昇格させてはいけない。**
                    // 「すき」+「いろ」は質問文から抜けるが、繋ぎ目に
                    // 「きいろ」が出来てしまう。毎回きまって Yellow に
                    // なり、後ろの本当の答えを遮る。
                    pending = match head_of_a_reading(&pending) {
                        // 読みとして出来上がっている。区間にしてよい。
                        Some(p) if is_a_reading(&p) => {
                            parts.push(p);
                            joinable = true;
                            Vec::new()
                        }
                        // 読みの途中。**ここで区間にすると、続きが
                        // 別の区間になって離れてしまう。** 溜めに残して
                        // 次と繋ぐ（「いろ き み どり」の「き」）。
                        Some(p) => vec![p],
                        // 読みの頭ですらない。回り込みとして捨てる。
                        None => Vec::new(),
                    };
                }
                continue;
            }
            if joinable {
                if let Some(prev) = parts.last_mut() {
                    prev.push_str(&seg);
                }
            }
            continue;
        }
        // 溜めがあれば、それは割れた語の前半かもしれない。**繋いだ結果が
        // どれかの読みの頭になるときだけ繋ぐ。**
        //
        // 捨てていた頃は「き みどり」の「き」が消えて みどり になった。
        // 無条件に繋ぐと、今度は回り込みが答えに貼り付く。
        //
        //     「すき みどり」→ すきみどり ⊃ きみどり
        //
        // **回り込んで残るのは質問の末尾のほう。** `listen` は窓を開ける
        // 瞬間にバッファを捨てるので、頭の「どんな」はたいてい残らない。
        // 質問が現れたかどうかで見分けようとすると、実際に起きる側を
        // 素通しすることになる。
        //
        // 質問文の字で読みの頭になれるのは「き」だけなので、この判定が
        // 効く範囲は狭い。
        // **溜めのうち末尾だけを持ち出さない。** 「す」「き」と割れた質問の
        // 「き」だけを繋ぐと、「す き みどり」が きみどり になる。丸ごと
        // 読みの頭になっているときだけ持ち出す（「き」→ きいろ / きみどり）。
        let carried = head_of_a_reading(&pending).filter(|p| *p == pending.concat());
        pending.clear();
        parts.push(match carried {
            None => seg,
            Some(p) => {
                let joined = p.clone() + &seg;
                // 読みの途中まで（「き」+「み」= きみ）か、読みが丸ごと
                // 頭にある（「き」+「いろだよ」= きいろ + だよ）か。
                //
                // 前者だけを見ていた頃は、答えに「だよ」「です」が付く
                // だけで繋がず、「き」を捨てて後半だけ判定していた。
                //
                //     き いろだよ => なし   （きいろ と言っている）
                //
                // 後者でも**読みの側が溜めから始まること**は求める。
                // 求めないと「すき みどり」が きみどり に貼り付く。
                let fits = CANDIDATES.iter().any(|(_, r)| {
                    r.starts_with(&joined) || (r.starts_with(&p) && joined.starts_with(r))
                });
                if fits {
                    joined
                } else {
                    seg
                }
            }
        });
        joinable = true;
    }
    // 同じ語の繰り返しは畳む。tiny が出力を繰り返すことがある。
    parts.dedup();
    parts.truncate(head.max(1));
    parts
}

/// 読みとして出来上がっているか。頭の途中と区別する。
fn is_a_reading(s: &str) -> bool {
    CANDIDATES.iter().any(|(_, r)| *r == s)
}

/// 破片の列の末尾から、**読みの頭になれる最長のもの**を返す。
///
/// 破片の境目を尊重するのが要点。列を繋いだ文字列の中を探すと、繋ぎ目に
/// 無かった語が湧く。
///
/// ```text
/// 「すき」「いろ」            → なし   （すきいろ も いろ も読みの頭ではない）
/// 「き」「いろ」              → きいろ
/// 「いろ」「が」「すき」「き」 → き
/// ```
///
/// 繋いだ文字列で探していた頃は、1つ目が「すきいろ ⊃ きいろ」で Yellow に
/// なっていた。「すき」の中の「き」は使ってはいけない。
fn head_of_a_reading(fragments: &[String]) -> Option<String> {
    // 長いほうから。末尾に寄るほど、質問の回り込みではなく答えに近い。
    (0..fragments.len()).find_map(|from| {
        let joined = fragments[from..].concat();
        CANDIDATES
            .iter()
            .any(|(_, r)| r.starts_with(&joined))
            .then_some(joined)
    })
}

/// **先頭から始まって境目をまたぐ**読みを探す。`boundary` は前の区間の
/// 終わり（バイト位置）。
///
/// またぐだけでは足りない。前の区間の末尾1文字を拾って後ろに繋がるだけで
/// 「またいだ」ことになり、**幻覚の継ぎ足しが先頭を追い越す**。
///
/// ```text
/// 「あき、みどり」  き + みどり = きみどり がまたぐ → きみどり
/// ```
///
/// 先頭から始まることまで求めれば、前の区間が丸ごと読みの一部だった場合
/// だけになる。狙いだった「むら さきいろ」はそちら。
fn straddling(text: &str, boundary: usize) -> Option<Answer> {
    CANDIDATES
        .iter()
        .find(|(_, r)| text.starts_with(*r) && r.len() > boundary)
        .map(|(a, _)| *a)
}

/// 句読点で区切る。
///
/// whisper が不明瞭な音を受け取ると、`initial_prompt` に渡した語彙を
/// そのまま並べて返すことがある。実際に「あお、きいろ、みどり、しろ、みど」
/// が出た。区切らずに繋げると、部分一致が長い読みから探すせいで
/// 途中の「きいろ」を拾ってしまう。区切って先頭から見れば、
/// 最初に言われた色を優先できる。
///
/// **ひらがな以外を境界にしてはいけない。** 漢字が区間から消えるので、
/// 「全部」がまるごと落ちる。実際にそうなった。
fn segments(s: &str) -> Vec<String> {
    s.split(is_separator)
        .filter(|seg| !seg.is_empty())
        .map(str::to_string)
        .collect()
}

fn is_separator(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '、' | '。'
                | '，'
                | '．'
                | ','
                | '.'
                | '!'
                | '?'
                | '！'
                | '？'
                | '「'
                | '」'
                | '（'
                | '）'
                | '('
                | ')'
                | '・'
                | '〜'
                | '~'
                | '/'
        )
}

/// カタカナをひらがなに寄せる。
///
/// **通すのは認識結果だけ。** 語彙の側は初めから正規形で書く約束なので、
/// カタカナ表記を並べる必要も、ここを通す必要もない。
/// 長音符「ー」はカタカナ領域の外なのでそのまま残る。
///
/// **空白も句読点も落とさない。** どちらも `segments` が区切りとして使う。
/// ここで空白を消していた頃は `is_separator` の空白判定が到達せず、
/// 「あお あか」が「あおあか」に繋がって、部分一致で後ろの色が勝っていた。
/// 先頭を優先する方針が空白入力で破れる。
fn normalize(s: &str) -> String {
    s.chars()
        // **長音符はここで落とす。** 骨格の側だけで落としていたので、
        // 完全一致と部分一致は生の文字列を見ることになり、扱いが割れて
        // いた。「きーみどり」は骨格なら きみどり に丸ごと当たるのに、
        // 部分一致が先に みどり を拾う。語尾が付くと今度はどちらにも
        // 届かず「あーかだよ」が丸ごと落ちる。
        //
        // 語彙にも質問文にも「ー」は入っていないので、落として困る側が無い。
        .filter(|&c| c != 'ー')
        .map(|c| {
            let u = c as u32;
            if (0x30A1..=0x30F6).contains(&u) {
                char::from_u32(u - 0x60).unwrap_or(c)
            } else {
                c
            }
        })
        .collect()
}

fn strip_prompt(s: &str) -> String {
    let mut out = s.to_string();
    for p in PROMPT {
        out = out.replace(p, "");
    }
    out
}

/// 五十音表。行の子音と、あいうえお順の並び。
/// `_` はその行に無い音。
const ROWS: &[(u8, &str)] = &[
    (b'-', "あいうえお"),
    (b'k', "かきくけこ"),
    (b's', "さしすせそ"),
    (b't', "たちつてと"),
    (b'n', "なにぬねの"),
    (b'h', "はひふへほ"),
    (b'm', "まみむめも"),
    (b'y', "や_ゆ_よ"),
    (b'r', "らりるれろ"),
    (b'w', "わ___を"),
    (b'g', "がぎぐげご"),
    (b'z', "ざじずぜぞ"),
    (b'd', "だぢづでど"),
    (b'b', "ばびぶべぼ"),
    (b'p', "ぱぴぷぺぽ"),
];

/// 仮名を（子音, 母音）に分ける。表に無ければ `None`。
fn decompose(c: char) -> Option<(u8, usize)> {
    ROWS.iter().find_map(|(consonant, row)| {
        row.chars()
            .position(|k| k == c)
            .map(|vowel| (*consonant, vowel))
    })
}

/// 混同しやすい子音の組。幼児の発音でも音声認識でも入れ替わる。
///
/// `sztd` をひとまとめにしているのは、い段で「し・じ・ち・ぢ」が
/// 硬口蓋の摩擦音／破擦音に寄って区別が付かなくなるため。
/// 実際に「ちゃいろ」が「じゃあ、いろ」と認識された。
const NEAR_CONSONANTS: &[&[u8]] = &[
    b"kg",   // か行・が行（清濁）
    b"sztd", // さ・ざ・た・だ行（清濁と、し/じ/ち/ぢ の混同）
    b"hbp",  // は・ば・ぱ行
    b"mn",   // 鼻音
    b"rd",   // ら行・だ行（幼児で入れ替わる）
];

/// 母音だけの音（あいうえお）を表す印。
const NO_CONSONANT: u8 = b'-';

fn consonant_cost(a: u8, b: u8) -> usize {
    if a == b {
        0
    } else if NEAR_CONSONANTS
        .iter()
        .any(|g| g.contains(&a) && g.contains(&b))
    {
        1
    } else if a == NO_CONSONANT || b == NO_CONSONANT {
        // 子音が丸ごと消える／生えるのは、別の子音に変わるより大きい変化。
        // ここを同じ扱いにすると「みじろ」が「みずいろ」ではなく
        // 「きいろ」に寄ってしまう（じ→い を安く見積もるため）。
        3
    } else {
        2
    }
}

/// 挿入・削除の重み。
///
/// 母音だけの音は落ちたり伸びたりしやすいので安くする。
/// 「みずいろ」が「みじろ」と認識されたように、幼児の発音でも
/// 認識結果でも、母音1つの脱落は頻繁に起きる。
fn indel(c: char) -> usize {
    match decompose(c) {
        Some((NO_CONSONANT, _)) => 2,
        _ => INDEL,
    }
}

/// 置換の重み。1音ぶんのずれが 4 になる目盛り。
///
/// 素の編集距離だと文字が違えば一律1で、「じ→ぜ」（同じザ行）と
/// 「ば→く」（無関係）が同じ扱いになる。子音と母音を別々に数え、
/// さらに子音は音の近さで刻む。
///   じ→ぜ  子音 z=z(0)      母音 i≠e(2)  → 2
///   じ→ち  子音 z≈t(1)      母音 i=i(0)  → 1
///   じ→き  子音 z≠k(2)      母音 i=i(0)  → 2
///   じ→い  子音 z→無し(3)   母音 i=i(0)  → 3
///   ば→く  子音 b≠k(2)      母音 a≠u(2)  → 4
fn substitution(a: char, b: char) -> usize {
    if a == b {
        return 0;
    }
    match (decompose(a), decompose(b)) {
        (Some((ca, va)), Some((cb, vb))) => consonant_cost(ca, cb) + if va == vb { 0 } else { 2 },
        // 「ん」や表に無い文字。似ている度合いを測れないので別物とみなす。
        _ => 4,
    }
}

/// 挿入・削除の重み。1音まるごとのずれを 4 とする目盛りに合わせる。
/// 置換の最大は 5（子音の消失3＋母音違い2）なので、そちらのほうが
/// わずかに高い。つまり同じ長さなら置換より挿入＋削除を選びにくい。
///
/// もとの意図は、
/// 「1音ぶんのずれ」がどの操作でも同じ値になるようにする。
const INDEL: usize = 4;

/// 音の近さで重み付けした編集距離。日本語なので文字単位で測る。
fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = std::iter::once(0)
        .chain(b.iter().scan(0, |acc, &c| {
            *acc += indel(c);
            Some(*acc)
        }))
        .collect();
    let mut cur: Vec<usize> = vec![0; b.len() + 1];
    for ca in a {
        cur[0] = prev[0] + indel(*ca);
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j + 1] + indel(*ca))
                .min(cur[j] + indel(*cb))
                .min(prev[j] + substitution(*ca, *cb));
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use crate::app::color::Color;

    use super::*;

    pub fn find(s: &str) -> Option<Answer> {
        Matcher::new(2).find(s)
    }
    fn c(x: Color) -> Option<Answer> {
        Some(Answer::Single(x))
    }

    /// 候補を `normalize` に通していないので、**語彙の側が正規形である
    /// ことが前提**になる。カタカナや空白が紛れ込むと、判定が黙って
    /// 当たらなくなる。制約が破れたらここで気づく。
    #[test]
    fn vocabulary_is_already_normalized() {
        for a in Answer::every() {
            assert_eq!(normalize(a.reading()), a.reading(), "読みが正規形でない");
            if let Some(k) = a.kanji() {
                assert_eq!(normalize(k), k, "漢字表記が正規形でない");
            }
        }
    }

    /// 部分一致は長いほうから試す。並びが崩れると「きみどりがすき」が
    /// 「みどり」に化ける。const で組んであるので、崩れたらここで気づく。
    #[test]
    fn candidates_are_sorted_longest_first() {
        let lens: Vec<usize> = CANDIDATES.iter().map(|(_, r)| r.chars().count()).collect();
        assert!(
            lens.windows(2).all(|w| w[0] >= w[1]),
            "長い順でない: {lens:?}"
        );

        // 同じ長さなら読みが先。`after` が元の位置まで見るので、並べ替えの
        // 安定性に関係なくこの順になる。ここで見ているのは「意図した順に
        // なっているか」であって、アルゴリズムの性質ではない。
        //
        // 崩れると「あかあお」のように繋がって認識されたとき、部分一致が
        // どちらを採るかが変わる。
        let is_kanji = |(a, r): &(Answer, &str)| a.kanji() == Some(*r);
        for w in CANDIDATES.windows(2) {
            if w[0].1.chars().count() == w[1].1.chars().count() && is_kanji(&w[0]) {
                assert!(is_kanji(&w[1]), "同じ長さで漢字のあとに読みが来た");
            }
        }

        // 同じ長さの読みどうしは宣言順。先に宣言した色が先に当たる。
        let two: Vec<&str> = CANDIDATES
            .iter()
            .filter(|c| c.1.chars().count() == 2 && !is_kanji(c))
            .map(|c| c.1)
            .collect();
        assert_eq!(two, ["あか", "あお", "しろ", "くろ"], "宣言順が崩れた");

        // const で数えた丈が、実際に数えたものと合っているか。
        let kanji = Answer::every()
            .iter()
            .filter(|a| a.kanji().is_some())
            .count();
        assert_eq!(CANDIDATES.len(), Answer::COUNT + kanji);
        assert_eq!(char_count("きみどり"), "きみどり".chars().count());
    }

    #[test]
    fn exact_match() {
        assert_eq!(find("あか"), c(Color::Red));
        assert_eq!(find("みずいろ"), c(Color::LightBlue));
        assert_eq!(find("ぜんぶ"), Some(Answer::All));
    }

    #[test]
    fn katakana_is_normalized_to_hiragana() {
        assert_eq!(find("アカ"), c(Color::Red));
        assert_eq!(find("ピンク"), c(Color::Pink));
        assert_eq!(find("オレンジ"), c(Color::Orange));
    }

    #[test]
    fn punctuation_and_space_are_stripped() {
        assert_eq!(find(" あか！ "), c(Color::Red));
        assert_eq!(find("あお。"), c(Color::Blue));
    }

    #[test]
    fn substring_prefers_the_longest_reading() {
        // 「きみどり」は「みどり」を含む。短いほうから見ると取りこぼす。
        assert_eq!(find("きみどりがすき"), c(Color::YellowGreen));
        assert_eq!(find("みどりだよ"), c(Color::Green));
    }

    #[test]
    fn fuzzy_match_absorbs_mispronunciation() {
        assert_eq!(find("むらさぎ"), c(Color::Purple));
        assert_eq!(find("みずいる"), c(Color::LightBlue));
    }

    #[test]
    fn closest_sounding_color_wins() {
        // 「あき」は「あか」（カ行の母音違い）のほうが
        // 「あお」（子音も母音も違う）より近い。
        assert_eq!(find("あき"), c(Color::Red));
    }

    #[test]
    fn all_absorbs_small_slips() {
        assert_eq!(find("ぜんぷ"), Some(Answer::All));
        assert_eq!(find("ぜんむ"), Some(Answer::All));
        // 実際に出た形。拗音と長音を落とすと「じんば」で距離2。
        assert_eq!(find("ジェンバー"), Some(Answer::All));
        assert_eq!(find("じゃんぶー"), Some(Answer::All));
    }

    #[test]
    fn all_does_not_fire_on_distant_words() {
        // 音が2つずれるものまで拾うとゲームが事故で終わる。
        assert_ne!(find("でんわ"), Some(Answer::All));
        assert_ne!(find("わんわん"), Some(Answer::All));
        assert_ne!(find("ごはん"), Some(Answer::All));
    }

    #[test]
    fn empty_input() {
        assert_eq!(find(""), None);
        assert_eq!(find("   "), None);
    }

    #[test]
    fn unrelated_speech_is_rejected() {
        assert_eq!(find("おかあさん"), None);
    }

    #[test]
    fn kanji_is_matched() {
        // initial_prompt は誘導であって強制ではないので漢字は出る。
        assert_eq!(find("全部"), Some(Answer::All));
        assert_eq!(find("赤"), c(Color::Red));
        assert_eq!(find("黄色"), c(Color::Yellow));
        assert_eq!(find("黄緑"), c(Color::YellowGreen));
        assert_eq!(find("水色"), c(Color::LightBlue));
    }

    #[test]
    fn prompt_echo_takes_the_first_color() {
        // whisper が initial_prompt の語彙を吐き返したときに実際に出た形。
        // 繋げて探すと途中の「きいろ」を拾ってしまう。
        assert_eq!(find("あお、きいろ、みどり、しろ、みど"), c(Color::Blue));
    }

    #[test]
    fn split_word_is_rejoined() {
        // 認識が「ちゃいろ」を「じゃあ、いろ」に割ったときの実例。
        // 後半だけ見ると「しろ」が最も近い。全体で見れば茶色が勝つ。
        assert_eq!(find("じゃあ、いろ。"), c(Color::Brown));

        // **前半がたまたま別の色に近いだけで確定してはいけない。**
        // 単独で3段を使い切ってから結合に移っていた頃は、「むら」が
        // くろ に、「きみ」が きいろ に音の近さで当たって、結合の
        // 完全一致まで届かなかった。
        assert_eq!(find("むら さき"), c(Color::Purple));
        assert_eq!(find("むら、さき"), c(Color::Purple));
        assert_eq!(find("きみ どり"), c(Color::YellowGreen));

        // **割れた語の1つ目が質問文の一部でも消してはいけない。**
        // 「き」は「すき」の一部。後ろ向きにしか繋がなかった頃は、
        // 繋ぎ先が無くて捨てられ、きいろ が丸ごと消えていた。
        assert_eq!(find("き、いろ"), c(Color::Yellow));
        assert_eq!(find("き いろ"), c(Color::Yellow));
        assert_eq!(find("みず いろ"), c(Color::LightBlue));
        assert_eq!(find("ちゃ いろ"), c(Color::Brown));

        // 幻覚の継ぎ足しを捨てる仕掛けは保つ。結合の部分一致を
        // 単独の音の近さより前に出すと、ここが きいろ になる。
        assert_eq!(find("みじろ、きいろ"), c(Color::LightBlue));
        assert_eq!(find("どんな いろ みじろ"), c(Color::LightBlue));

        // **割れたうえに一音ずれると、結合しても完全一致には届かない。**
        // 単独と結合を突き合わせて近いほうを採る。
        //   むら → くろ d=4 / むらさぎ → むらさき d=1
        assert_eq!(find("むら さぎ"), c(Color::Purple));

        // 溜めた破片は、次が質問の続きでなくても繋ぐ。捨てていた頃は
        // 「き」が消えて みどり になった。
        assert_eq!(find("き みどり"), c(Color::YellowGreen));
        assert_eq!(find("き み どり"), c(Color::YellowGreen));
        // ぜんぶ も割れる。ここを取りこぼすと遊びが終われない。
        assert_eq!(find("ぜん ぶ"), Some(Answer::All));

        // **繋ぐのは読みの頭になるときだけ。** 無条件に繋いでいた頃は、
        // 回り込みが答えに貼り付いて毎回きまって別の色が鳴った。
        //   すきみどり ⊃ きみどり
        assert_eq!(find("すき みどり"), c(Color::Green));
        assert_eq!(find("いろ が すき みどり"), c(Color::Green));

        // **回り込んだうえに割れても拾う。** 質問が現れたら破片を捨てる、
        // としていた頃はここが丸ごと消えていた。
        assert_eq!(find("どんな いろ き いろ"), c(Color::Yellow));
        assert_eq!(find("どんな いろ が すき き いろ"), c(Color::Yellow));
        assert_eq!(find("どんな いろ が すき き みどり"), c(Color::YellowGreen));

        // 境目をまたぐ部分一致は、割れ方が誤りだった証拠として採る。
        // 「紫色」は「紫」を含むので拾える、が割れても保たれる。
        assert_eq!(find("むら さきいろ"), c(Color::Purple));
        assert_eq!(find("むらさきいろ"), c(Color::Purple));
    }

    #[test]
    fn dropped_vowel_is_cheap() {
        // 「みずいろ」が「みじろ」と認識された実例。母音の脱落を
        // 高く見積もると「きいろ」に負ける。
        assert_eq!(find("みじろ"), c(Color::LightBlue));
        assert_eq!(find("みじろ、みじろ。"), c(Color::LightBlue));
    }

    #[test]
    fn hallucinated_tail_is_ignored() {
        // tiny は言っていない語を後ろに継ぎ足す。先頭を優先する。
        assert_eq!(find("あお、おれんじ、あか"), c(Color::Blue));
        assert_eq!(find("あか、あお、みどり、しろ、くろ"), c(Color::Red));
        // 先頭が3段目でしか当たらなくても、後ろの完全一致に譲らない。
        assert_eq!(find("みじろ、みじろ、きいろ、しろ"), c(Color::LightBlue));
        assert_eq!(find("みじろ、きいろ"), c(Color::LightBlue));
    }

    #[test]
    fn earlier_segment_wins() {
        assert_eq!(find("あか。あお"), c(Color::Red));
        assert_eq!(find("むらさき / みどり"), c(Color::Purple));
        // 空白も区切り。繋げてしまうと部分一致で後ろの色が勝つ。
        assert_eq!(find("あお あか"), c(Color::Blue));
        assert_eq!(find("みどり\tきいろ"), c(Color::Green));
    }

    #[test]
    fn question_bleeding_in_does_not_become_a_color() {
        // 質問が回り込んでも色にしない。「いろ」を残すときいろに化ける。
        assert_eq!(find("どんないろがすき"), None);
        // 質問の後ろに答えがくっついても答えだけ拾う。
        assert_eq!(find("どんないろがすきあか"), c(Color::Red));

        // **whisper は質問を空白で割って返す。** 区切ってから削るので、
        // 破片の「いろ」が生き残って「しろ」に化けていた。
        assert_eq!(find("どんな いろ"), None);
        assert_eq!(find("いろ"), None);
        assert_eq!(find("どんな いろ が すき"), None);
        // **そして位置を最優先するので、破片が本当の答えを押し出す。**
        // ランダムに外すのではなく、毎回きまって「しろ」が鳴っていた。
        assert_eq!(find("どんな いろ あか"), c(Color::Red));
        assert_eq!(find("どんな いろ が すき みどり"), c(Color::Green));

        // **前に何か挟まっても同じこと。** 破片を直前へ繋ぐようにしたら、
        // 今度は繋ぎ先がフィラーになって「ちゃいろ」「きみどり」に化けた。
        // 質問が挟まった時点で繋ぎ先を切る。
        assert_eq!(find("じゃあ どんな いろ あか"), c(Color::Red));
        assert_eq!(find("じゃあ どんな いろ が すき みどり"), c(Color::Green));
        assert_eq!(find("えー どんな いろ が すき みどり"), c(Color::Green));
    }

    /// 1音のフィラーは色にしない。
    ///
    /// **短すぎるものは何にでも近い。** 「えー」の骨格は「え」1文字で、
    /// 「あお」に一意に当たっていた。位置が最優先なので、後ろで本当に
    /// 言った色を押し出す。ランダムに外すのではなく毎回きまって外す。
    #[test]
    fn a_single_mora_is_not_a_color() {
        assert_eq!(find("えー"), None);
        assert_eq!(find("えー みどり"), c(Color::Green));
        assert_eq!(find("うーん あか"), c(Color::Red));
        // 下限の根拠。候補の骨格はどれも2文字以上ある。
        for (a, sk) in Matcher::new(2).skeletons.iter() {
            assert!(sk.len() >= 2, "{a:?} の骨格が2文字未満: {sk:?}");
        }
    }

    /// 質問文の破片を捨てる仕掛けが、答えのほうを巻き込んでいないこと。
    ///
    /// 色を足したときに「読みがたまたま質問文の一部だった」となると、
    /// その色だけ永久に当たらなくなる。無言に倒れるので気づきにくい。
    #[test]
    fn no_answer_hides_inside_the_question() {
        for (answer, reading) in CANDIDATES {
            assert!(
                !QUESTION.contains(reading),
                "{answer:?} の読み「{reading}」が質問文の一部になっている。\
                 このままだと聞き取れても捨てられる"
            );
        }
    }

    /// 表で回す回帰。**ラウンドを重ねて集まった実例をここに集約する。**
    ///
    /// 段の順や前処理の規則をいじると、直したいものとは別のところが
    /// 動く。個別の assert だと最初の1件で止まって全体像が見えないので、
    /// 全部試してからまとめて出す。
    ///
    /// 期待値は読みで書く。`-` は「判定しない」（呼び出し側でランダムな色）。
    #[test]
    fn the_cases_that_taught_us_something() {
        // 入力, 期待する読み, なぜこれが表にあるか
        const TABLE: &[(&str, &str, &str)] = &[
            ("あか", "あか", "素直な一致"),
            ("ぜんぶ", "ぜんぶ", "終わりの合図"),
            ("みじろ", "みずいろ", "母音の脱落"),
            ("きーろ", "-", "きいろ / しろ / くろ が三つ巴で同点。諦める"),
            ("しーろ", "しろ", "長音は骨格で落とす"),
            ("どんないろがすき", "-", "質問の回り込み"),
            // 認識が語を割る
            ("じゃあ、いろ。", "ちゃいろ", "ちゃいろ が割れた実例"),
            (
                "むら さき",
                "むらさき",
                "結合の完全一致が単独の音の近さに負けていた",
            ),
            ("きみ どり", "きみどり", "同上"),
            (
                "むら さぎ",
                "むらさき",
                "割れたうえに一音ずれる。距離で決める",
            ),
            ("ぜん ぶ", "ぜんぶ", "終われなくなる"),
            ("むら さきいろ", "むらさき", "境目をまたぐ部分一致"),
            ("むらさきいろ", "むらさき", "割れていない同じもの"),
            // 質問の回り込みと、割れた答えの区別
            (
                "どんな いろ",
                "-",
                "空白で割れると strip_prompt を擦り抜けていた",
            ),
            ("いろ", "-", "破片は単独で答えにならない"),
            (
                "どんな いろ あか",
                "あか",
                "破片が本当の答えを押し出していた",
            ),
            ("どんな いろ が すき みどり", "みどり", "同上"),
            (
                "じゃあ どんな いろ あか",
                "あか",
                "繋ぎ先がフィラーになっていた",
            ),
            ("すき みどり", "みどり", "回り込みが答えに貼り付いていた"),
            ("いろ が すき みどり", "みどり", "同上"),
            (
                "き いろ",
                "きいろ",
                "割れた語の頭が質問文の一部（き ⊂ すき）",
            ),
            (
                "き みどり",
                "きみどり",
                "同上。次が質問の続きでなくても繋ぐ",
            ),
            ("き み どり", "きみどり", "3つに割れる"),
            ("き いろだよ", "きいろ", "読みの後ろに語が付いても繋ぐ"),
            ("き みどりだよ", "きみどり", "同上"),
            ("みず いろ", "みずいろ", "同上"),
            ("ちゃ いろ", "ちゃいろ", "同上"),
            ("どんな いろ き いろ", "きいろ", "回り込んだうえに割れる"),
            ("どんな いろ が すき き いろ", "きいろ", "同上"),
            ("どんな いろ が すき き みどり", "きみどり", "同上"),
            (
                "どんな いろ みじろ",
                "みずいろ",
                "破片を答えに繋いではいけない側",
            ),
            // 幻覚の継ぎ足しは捨てる。位置が最優先
            ("あお、おれんじ、あか", "あお", "後ろに継ぎ足す"),
            (
                "みじろ、きいろ",
                "みずいろ",
                "先頭は音の近さでしか当たらないが、それでも先頭",
            ),
            (
                "みじろ、みじろ、きいろ、しろ",
                "みずいろ",
                "繰り返し＋継ぎ足し",
            ),
            (
                "あき、みどり",
                "あか",
                "末尾1文字が継ぎ足しに繋がって、またいで見えた",
            ),
            ("むらざき、みどり", "むらさき", "同上"),
            ("あか。あお", "あか", "先頭優先"),
            ("あお あか", "あお", "空白も区切り"),
            // 溜めの継ぎ目から語が湧かないこと
            (
                "すき いろ",
                "-",
                "繋いだ文字列で探すと継ぎ目に きいろ が出来る",
            ),
            (
                "すき いろ あか",
                "あか",
                "同上。湧いた色が本当の答えを遮っていた",
            ),
            ("どんな いろ が すき いろ あか", "あか", "同上"),
            ("いろ き みどり", "きみどり", "破片の境目を尊重する"),
            (
                "いろ き み どり",
                "きみどり",
                "読みの途中を区間にすると続きが離れる",
            ),
            ("いろ き みど り", "きみどり", "同上"),
            (
                "どんな いろ が すき き み どり",
                "きみどり",
                "同上。回り込み付き",
            ),
            (
                "じゃあ おれんじ",
                "おれんじ",
                "またぐ判定が、右で完結した答えを遮っていた",
            ),
            // フィラー
            ("えー", "-", "1音は何にでも近い"),
            ("あの", "-", "2モーラのフィラー。あお に d=3 で当たっていた"),
            (
                "あの、あか",
                "あか",
                "「あの」は2歳児の言い出しの定番。毎回 あお になっていた",
            ),
            ("この", "-", "3音の読みへ1音半まで許すと きいろ に当たる"),
            ("この あお", "あお", "同上。短いほうで測る"),
            ("その", "-", "しろ に d=4 で当たっていた"),
            (
                "えー ちろ",
                "しろ",
                "結合の音の近さが、次の区間単独の確かな一致を潰していた",
            ),
            ("えー かろ", "くろ", "同上"),
            ("みずろ", "みずいろ", "割れていない形"),
            (
                "み ずろ",
                "みずいろ",
                "同点で次に譲ると、割れ方だけで別の色になっていた",
            ),
            ("きみ どどり", "きみどり", "同上"),
            ("あの つゃいろ", "ちゃいろ", "同上"),
            // 長音符
            (
                "きーみどり",
                "きみどり",
                "骨格だけで落としていたので、部分一致が先に みどり を拾った",
            ),
            ("あーかだよ", "あか", "語尾が付くとどの段にも届かなかった"),
            ("ぴーんくだよ", "ぴんく", "同上"),
            // 言い切り
            (
                "みず",
                "みずいろ",
                "2歳児は「みず！」と言う。読みの頭そのものなら言い切り",
            ),
            ("みど", "みどり", "同上"),
            ("ぜん", "ぜんぶ", "同上。これは終わってよい"),
            // 溜めの持ち出し
            (
                "す き みどり",
                "みどり",
                "質問が割れた「き」だけを持ち出さない",
            ),
            (
                "す き いろ",
                "きいろ",
                "**現状こうなる。** 割れ方が曖昧で、どちらとも決めきれない",
            ),
            ("えー みどり", "みどり", "1音が本当の答えを押し出していた"),
            ("うーん あか", "あか", "同上"),
        ];

        let mut ng: Vec<String> = Vec::new();
        for (input, want, why) in TABLE {
            let got = match find(input) {
                Some(Answer::All) => Answer::All.reading().to_string(),
                Some(Answer::Single(c)) => c.reading().to_string(),
                None => "-".to_string(),
            };
            if got != *want {
                ng.push(format!("  {input:?} => {got}（期待 {want}）\n    {why}"));
            }
        }
        assert!(ng.is_empty(), "\n{}", ng.join("\n"));
    }

    /// 判定にかける前に、区間の列がどう組み上がるか。
    ///
    /// **ここは状態機械。** 溜め・繋ぎ先・質問の破片という3つの規則が絡み、
    /// 直近の穴はどれも個別の入力ではなく規則同士の組み合わせで出た。
    /// 判定を通した結果だけ見ていると、組み立てのどこが効いたのか分からない。
    #[test]
    fn assembles_the_segments() {
        // 入力, 出来上がる区間, なぜこれが表にあるか
        const TABLE: &[(&str, &[&str], &str)] = &[
            ("あか", &["あか"], "素直"),
            ("あお あか", &["あお", "あか"], "空白も区切り"),
            ("じゃあ、いろ。", &["じゃあいろ"], "破片を直前へ繋ぐ"),
            ("どんな いろ", &[], "質問はまるごと消える"),
            ("どんな いろ あか", &["あか"], "破片が席を占めない"),
            (
                "じゃあ どんな いろ あか",
                &["じゃあ", "あか"],
                "質問が挟まったら繋ぎ先を切る",
            ),
            (
                "どんな いろ みじろ",
                &["みじろ"],
                "読みの頭でない溜めは繋がない",
            ),
            ("き いろ", &["きいろ"], "溜めが読みとして出来上がる"),
            ("き いろだよ", &["きいろだよ"], "読みが丸ごと頭にある"),
            ("き み どり", &["きみ", "どり"], "読みの途中は溜めに残す"),
            (
                "いろ き み どり",
                &["きみ", "どり"],
                "回り込みを剥がしてから同じことをする",
            ),
            ("いろ き", &[], "読みの途中のまま終われば捨てる"),
            (
                "すき いろ あか",
                &["あか"],
                "繋ぎ目に湧いた語を昇格させない",
            ),
            ("すき みどり", &["みどり"], "回り込みは答えに貼り付かない"),
            (
                "みじろ、みじろ、きいろ、しろ",
                &["みじろ", "きいろ"],
                "畳んで、先頭だけ信用する",
            ),
        ];

        let mut ng: Vec<String> = Vec::new();
        for (input, want, why) in TABLE {
            let got = assemble(input, 2);
            if got != *want {
                ng.push(format!(
                    "  {input:?} => {got:?}（期待 {want:?}）\n    {why}"
                ));
            }
        }
        assert!(ng.is_empty(), "\n{}", ng.join("\n"));
    }

    /// 1音ずらした言い方を機械で作って、崩れ方を見張る。
    ///
    /// 表は起きたことを覚えておくもので、**まだ起きていない崩れ方は拾えない。**
    /// 幼児の発音は毎回ずれるので、ずらしたものを網羅して掛ける。
    ///
    /// 見張るのは2つ。
    ///
    /// - **色の言い間違いが「ぜんぶ」になってはいけない。** これだけは
    ///   取り返しがつかない。遊びがそこで終わる
    /// - 別の色に化ける数。取りこぼし（無反応 → ランダムな色）は許すが、
    ///   化けるほうは「きまって同じ間違い」になるので数を抑えたい
    #[test]
    fn mispronunciations_never_end_the_game() {
        let m = Matcher::new(2);

        // 1文字落とす / 隣と入れ替える / 伸ばす（同じ字を重ねる）。
        let mutations = |s: &str| -> Vec<String> {
            let cs: Vec<char> = s.chars().collect();
            let mut out = Vec::new();
            for i in 0..cs.len() {
                let mut drop = cs.clone();
                drop.remove(i);
                out.push(drop.into_iter().collect());

                let mut twice = cs.clone();
                twice.insert(i, cs[i]);
                out.push(twice.into_iter().collect());

                if i + 1 < cs.len() {
                    let mut swap = cs.clone();
                    swap.swap(i, i + 1);
                    out.push(swap.into_iter().collect());
                }
            }
            out.retain(|v: &String| !v.is_empty());
            out
        };

        let mut fatal: Vec<String> = Vec::new();
        // どの読みが、どの読みに化けたか。
        let mut wrong: Vec<(&str, &str)> = Vec::new();
        let mut total = 0;

        for color in <Color as strum::VariantArray>::VARIANTS {
            for v in mutations(color.reading()) {
                // 前後に何か付く形と、**区間が割れた形**も見る。実際そう届く。
                //
                // 割れた形を入れていなかったので、「同点で次に譲る」の
                // 取り違え（同じ発話が割れ方だけで別の色になる）を作れて
                // いなかった。
                let split: Vec<String> = (1..v.chars().count())
                    .map(|at| {
                        let (a, b) = v.split_at(v.char_indices().nth(at).unwrap().0);
                        format!("{a} {b}")
                    })
                    .collect();
                let forms = [v.clone(), format!("えー {v}"), format!("{v}だよ")]
                    .into_iter()
                    .chain(split);
                for input in forms {
                    total += 1;
                    match m.find(&input) {
                        // これだけは絶対に起きてはいけない。
                        Some(Answer::All) => fatal.push(format!("  {input:?} => ぜんぶ")),
                        Some(Answer::Single(got)) if got != *color => {
                            wrong.push((color.reading(), got.reading()))
                        }
                        _ => {}
                    }
                }
            }
        }

        assert!(
            fatal.is_empty(),
            "色の言い間違いが「ぜんぶ」になった。遊びが事故で終わる:\n{}",
            fatal.join("\n")
        );

        // 化ける形の見張り。**数だけ見ていると、入れ替わったのを見逃す。**
        // どの読みがどの読みに化けたかで数える。
        //
        // ここに載っているのは、どれも人間が聞いても割れるもの。
        // 「みどり」は きみどり の脱落形でもある、という類。
        // **増えたり、新しい組が出たら、許してよいのか考えること。**
        const ALLOWED: &[(&str, &str, usize)] = &[
            // 「みどり」は きみどり に丸ごと入っている。頭が崩れれば
            // そちらに落ちる。どちらも緑なので、外れ方として一番軽い。
            ("きみどり", "みどり", 20),
            // 骨格にすると ちいろ / しろ で1音違い。人が聞いても割れる。
            ("ちゃいろ", "しろ", 4),
            // みずいろ の頭が崩れると ちゃいろ の骨格に寄る。
            ("みずいろ", "ちゃいろ", 4),
            // 以下は**二重に崩れた形**（重複＋区間割れ）。
            // 「ああ か」「おお れんじ」「みい ずろ」。
            ("あか", "あお", 1),
            ("おれんじ", "あお", 1),
            ("みずいろ", "くろ", 1),
        ];

        let mut seen: Vec<(&str, &str, usize)> = Vec::new();
        for (want, got) in &wrong {
            match seen.iter_mut().find(|(w, g, _)| w == want && g == got) {
                Some((_, _, n)) => *n += 1,
                None => seen.push((want, got, 1)),
            }
        }
        let key = |&(w, g, n): &(&str, &str, usize)| (usize::MAX - n, w.to_string(), g.to_string());
        seen.sort_by_key(key);
        let mut expected = ALLOWED.to_vec();
        expected.sort_by_key(key);
        assert_eq!(
            seen, expected,
            "\n化け方が変わった（全 {total} 通り）。左が実際、右が許しているもの"
        );
        eprintln!(
            "  ずらした言い方 {total} 通り: ぜんぶ誤爆 0 / 別の色 {}",
            wrong.len()
        );
    }

    #[test]
    fn levenshtein_weighs_by_sound() {
        let d = |x: &str, y: &str| {
            let a: Vec<char> = x.chars().collect();
            let b: Vec<char> = y.chars().collect();
            levenshtein(&a, &b)
        };
        assert_eq!(d("あか", "あか"), 0);
        // 同じ行なら母音のずれだけ
        assert_eq!(d("じ", "ぜ"), 2);
        assert_eq!(d("ば", "ぶ"), 2);
        // 近い子音は安い
        assert_eq!(d("じ", "ち"), 1);
        assert_eq!(d("ぱ", "ば"), 1);
        // 子音が丸ごと消えるのは、別の子音に変わるより大きい
        assert!(d("じ", "い") > d("じ", "き"));
        // 子音も母音も違えば満額
        assert_eq!(d("ば", "く"), 4);
        // 「じんば」は「ぴんく」より「ぜんぶ」に近い
        assert!(d("じんば", "ぜんぶ") < d("じんば", "ぴんく"));
    }
}
