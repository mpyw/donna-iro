#!/usr/bin/env bash
# Raspberry Pi 5 向けに組んで、そのまま配る。
#
#   tools/build-pi.sh              組むだけ → target/pi/donna-iro
#   tools/build-pi.sh pi5          組んで ssh 先の ~/donna-iro へ配る
#   tools/build-pi.sh pi5 /opt/x   配る先を変える
#
#   FEATURES=whisper tools/build-pi.sh    feature を変える（既定は既定 feature）
#
# **sysroot を使うクロスコンパイルはしない。** macOS から
# aarch64-unknown-linux-gnu を直接狙うと、whisper.cpp（cmake + bindgen）と
# ALSA のために sysroot・クロス gcc/g++・libclang のターゲット指定が要る。
# 手間の割に静かに壊れる。
#
# 代わりに **arm64 の Debian コンテナの中でネイティブに組む。** Mac が
# Apple Silicon なら linux/arm64 はエミュレーションなしで動くので、
# whisper.cpp 込みで1分を切る。イメージを Pi と同じ trixie に固定して
# あるので glibc も一致する（合わないと Pi 側で GLIBC_2.xx not found）。
#
# **Intel Mac ではエミュレーションになって現実的な速さにならない。**
# そのときは Pi 側で普通に cargo build したほうがよい。下で警告を出す。

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

DEST="${1:-}"
# **相対パスで持つ。** `~/donna-iro` と書くと手元のシェルが先に展開して
# Mac のホームになる。相対なら ssh 先のホームからの相対として素直に効く。
DEST_DIR="${2:-donna-iro}"
IMAGE="donna-iro-pi-builder"
# Pi の OS。**ここを Pi より新しくしてはいけない**（glibc が前方互換でない）。
BASE="debian:trixie-slim"
# 名前つきボリューム。**macOS 側の target/ と混ぜてはいけない。**
# 同じディレクトリを共有すると、ホストとコンテナでビルドが互いを
# 作り直し続けて毎回フルビルドになる。
VOL_TARGET="donna-iro-pi-target"
VOL_CARGO="donna-iro-pi-cargo"

command -v docker >/dev/null 2>&1 || {
  echo "docker が無い。" >&2
  echo "Intel Mac や docker を入れたくない場合は、Pi 側で cargo build するほうが早い:" >&2
  echo "  sudo apt install cmake libclang-dev libasound2-dev pkg-config" >&2
  exit 1
}

case "$(uname -m)" in
arm64 | aarch64) ;;
*)
  echo "⚠ ホストが $(uname -m) なので linux/arm64 はエミュレーションになる。" >&2
  echo "  whisper.cpp のビルドが現実的な時間で終わらない。Pi 側で組むことを勧める。" >&2
  ;;
esac

# --- builder イメージ ---
#
# **mise.toml も tools/check.sh も、ここでは使えない。** あれは macOS の
# 手元を見るためのもので、コンテナの中には別に道具を揃える必要がある。
# 中身は Cargo.toml と README が要求しているものと同じ:
#   cmake      whisper.cpp のビルド
#   libclang   bindgen が dlopen する
#   libasound2-dev  cpal（**どの feature でも入る**ので常に必要）
#   x11 / xkbcommon / wayland  minifb（window feature）
#
# rust は Pi に入れない。ここに閉じ込めておけば Pi 側は実行するだけで済む。
echo "builder イメージを用意（初回だけ数分）"
docker build --platform linux/arm64 -t "$IMAGE" - >/dev/null <<DOCKERFILE
FROM $BASE
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl git pkg-config \
      build-essential cmake \
      libclang-dev clang \
      libasound2-dev \
      libx11-dev libxkbcommon-dev libwayland-dev \
    && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo
ENV PATH=/usr/local/cargo/bin:\$PATH
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
WORKDIR /src
DOCKERFILE

# --- ビルド ---
#
# **埋め込み（embed）は使わない。** Pi ではバイナシの隣にファイルを置けるので、
# 141MB のモデルを焼き込む理由が無い。差し替えも rsync だけで済む。
# 埋め込みが要るのは CWD が / になる macOS の .app のほう。
FEATURES="${FEATURES:-}"
CARGO_ARGS=(build --release)
[ -n "$FEATURES" ] && CARGO_ARGS+=(--no-default-features --features "$FEATURES")

echo "ビルド中（whisper.cpp を組むので初回は1分ほど）"
docker run --rm --platform linux/arm64 \
  -v "$ROOT":/src \
  -v "$VOL_TARGET":/target \
  -v "$VOL_CARGO":/usr/local/cargo/registry \
  -e CARGO_TARGET_DIR=/target \
  "$IMAGE" \
  cargo "${CARGO_ARGS[@]}"

mkdir -p target/pi
docker run --rm --platform linux/arm64 \
  -v "$VOL_TARGET":/target -v "$ROOT/target/pi":/out \
  "$IMAGE" cp /target/release/donna-iro /out/donna-iro

BIN="target/pi/donna-iro"
echo
echo "できた: $ROOT/$BIN ($(du -h "$BIN" | cut -f1))"

if [ -z "$DEST" ]; then
  echo
  echo "配るには ssh 先を渡す:"
  echo "  tools/build-pi.sh pi5"
  exit 0
fi

# --- 配る ---
#
# **Pi から private サブモジュールは取れない。** assets/ は
# git@github.com の private リポジトリなので、Pi に鍵を置いて
# known_hosts を通すまで clone できない。手元から送るほうが早い。
#
# **macOS 同梱の rsync は古くて --info= を知らない。** 進捗を出したければ
# --progress を使うこと。
echo
echo "$DEST:$DEST_DIR へ配る"

# **rsync が作るのは行き先の最後の1階層だけ。** assets/reference は2階層
# 深いので、ここで掘っておかないと "No such file or directory" で落ちる。
# shellcheck disable=SC2029  # $DEST_DIR は手元で展開して送るのが意図
ssh "$DEST" "mkdir -p '$DEST_DIR/models' '$DEST_DIR/assets/reference'"

# 実行時に要る共有ライブラリ。X11 系は minifb が dlopen するので
# readelf の NEEDED には出ない。ここで名前を出して先に気づく。
#
# **ldconfig は /sbin にあり、ssh の非ログインシェルでは PATH に無い。**
missing="$(ssh "$DEST" '
  for l in libasound.so.2 libstdc++.so.6 libX11.so.6 libxkbcommon.so.0; do
    /sbin/ldconfig -p 2>/dev/null | grep -q "$l" || echo "$l"
  done')"
if [ -n "$missing" ]; then
  echo "⚠ 実行時ライブラリが足りない:" >&2
  while read -r l; do echo "    $l" >&2; done <<<"$missing"
  echo "  sudo apt install libasound2 libstdc++6 libx11-6 libxkbcommon0" >&2
fi

rsync -a "$BIN" "$DEST:$DEST_DIR/donna-iro"
rsync -a config.toml "$DEST:$DEST_DIR/config.toml"

# 本番音源があればそれ、無ければ確認用の合成音。**実行時のフォールバックに
# 任せる**ので、reference だけ送れば assets/ が空でも合成音で鳴る。
# _demo-* は通しの確認用で 10MB あるので送らない。
if ls assets/*.wav >/dev/null 2>&1; then
  echo "  本番音源を送る"
  rsync -a --exclude='_demo-*' assets/*.wav "$DEST:$DEST_DIR/assets/"
elif ls assets/reference/*.wav >/dev/null 2>&1; then
  echo "  本番音源が無いので確認用の合成音を送る"
  rsync -a --exclude='_demo-*' assets/reference/ "$DEST:$DEST_DIR/assets/reference/"
else
  echo "⚠ 音源が無い。git -C assets lfs pull を先に。" >&2
fi

# モデルは config.toml の paths.model が空なら models/ggml-base.bin。
if ls models/*.bin >/dev/null 2>&1; then
  echo "  モデルを送る（大きいので初回は待つ）"
  rsync -a models/*.bin "$DEST:$DEST_DIR/models/"
else
  echo "⚠ モデルが無い。tools/fetch-model.sh を先に。" >&2
fi

# shellcheck disable=SC2029  # $DEST_DIR は手元で展開して送るのが意図
ssh "$DEST" "chmod +x '$DEST_DIR/donna-iro'"

# --- OS 側の設定 ---
#
# **バイナリに入れられないもの。** 音量つまみ（softvol）と全画面の
# ウィンドウルールは OS の側の設定なので、外に出るしかない。手で置くと
# Git の外に出て次に再現できないので、pi/ で持ってここから配る。
# 何のための設定かは pi/README.md を見ること。
#
# **黙って上書きはしない。** 中身が違うものが既にあれば退避してから置く。
# 手で調整したものを消したことに気づけないのが一番たちが悪い。
# shellcheck disable=SC2029  # ssh に渡す変数は手元で展開して送るのが意図
deploy_conf() {
  local src="$1" dst="$2" reload="${3:-}"
  # 同じなら何もしない（毎回「置き換えた」と言わせない）。
  if ssh "$DEST" "[ -f '$dst' ]" &&
    ssh "$DEST" "cat '$dst'" 2>/dev/null | diff -q - "$ROOT/$src" >/dev/null 2>&1; then
    echo "  $dst は同じ"
    return 0
  fi
  if ssh "$DEST" "[ -f '$dst' ]"; then
    ssh "$DEST" "cp '$dst' '$dst.bak'"
    echo "  ⚠ $dst が違ったので $dst.bak に退避した"
  fi
  ssh "$DEST" "mkdir -p \"\$(dirname '$dst')\""
  rsync -a "$ROOT/$src" "$DEST:$dst"
  echo "  $dst を置いた"
  [ -n "$reload" ] && ssh "$DEST" "$reload" 2>/dev/null &&
    echo "    読み直させた: $reload"
  return 0
}

echo "OS 側の設定（pi/）"
deploy_conf pi/asoundrc .asoundrc
# labwc は SIGHUP で読み直す。動いていなければ何も起きないだけ。
deploy_conf pi/labwc-rc.xml .config/labwc/rc.xml "killall -HUP labwc"

echo
# shellcheck disable=SC2029  # $DEST_DIR は手元で展開して送るのが意図
echo "完了: $DEST:$DEST_DIR ($(ssh "$DEST" "du -sh '$DEST_DIR'" | cut -f1))"
echo "  ssh $DEST 'cd $DEST_DIR && ./donna-iro --terminal --keyboard'"
echo
echo "ウィンドウを開くにはデスクトップにログインしていること（コンソールでは開けない）。"
