#!/usr/bin/env bash
# whisper のモデルを models/ に取ってくる。
#
#   tools/fetch-model.sh          tiny（既定・74MB）
#   tools/fetch-model.sh base     大きいが精度が上がる（141MB）
#
# tiny で足りることは確認済み。語彙が12色に閉じていて initial_prompt で
# 誘導し、さらに編集距離で吸収するので、大きいモデルを積む理由がない。
#
# モデルはリポジトリには置かない（LFS の無駄）。
# 使うモデルは DONNA_IRO_MODEL で切り替える。
#
#   DONNA_IRO_MODEL=models/ggml-tiny.bin cargo run

set -euo pipefail

MODEL="${1:-tiny}"
case "$MODEL" in
  tiny | base | small | medium | large-v3 | large-v3-turbo) ;;
  *)
    echo "知らないモデル: $MODEL" >&2
    echo "使えるのは: tiny base small medium large-v3 large-v3-turbo" >&2
    exit 1
    ;;
esac

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIR="$ROOT/models"
FILE="$DIR/ggml-$MODEL.bin"
URL="https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-$MODEL.bin"

if [ -s "$FILE" ]; then
  echo "すでにある: $FILE ($(du -h "$FILE" | cut -f1))"
  exit 0
fi

mkdir -p "$DIR"
echo "取得中: $URL"
# 途中で失敗したものを掴まないよう、書き上がってから置き換える。
curl -fL --progress-bar -o "$FILE.part" "$URL"

# ggml のマグリック（リトルエンディアンで "lmgg"）を確かめる。
# 認証やリダイレクトに失敗すると HTML が落ちてくることがあり、
# それをモデルとして読ませると意味の分からないエラーになる。
MAGIC="$(head -c 4 "$FILE.part" | od -An -tx1 | tr -d ' \n')"
if [ "$MAGIC" != "6c6d6767" ]; then
  rm -f "$FILE.part"
  echo "ggml ファイルではない（先頭4バイト: $MAGIC）。落とし直してください。" >&2
  exit 1
fi

mv "$FILE.part" "$FILE"
echo "完了: $FILE ($(du -h "$FILE" | cut -f1))"

if [ "$MODEL" != "tiny" ]; then
  echo
  echo "既定は models/ggml-tiny.bin です。これを使うには:"
  echo "  DONNA_IRO_MODEL=$FILE cargo run"
fi
