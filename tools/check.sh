#!/usr/bin/env bash
# 全 feature の組み合わせでコンパイルを通し、テストと書式を確かめる。
#
#   tools/check.sh
#
# **feature を1つ落とすと静かに壊れる。** 実際に、実装をファイルへ分けた
# ときに `mic` の whisper と `channel` の window の cfg を落としていて、
# 既定のビルドだけ見ていたら気づけなかった。組み合わせを回すのはそのため。
#
# **エラーはどの組み合わせでも落とす。警告は既定のビルドだけ落とす。**
# 機能を削ったビルドでは、その構成で使われないものが素直に死ぬ（描画専用の
# Rgb、録音専用のアクセサなど）。これを0にするには app の側に io の feature
# 名で cfg を書くことになり、層が濁る。数は出すので、増えたら見ること。

set -uo pipefail
cd "$(dirname "$0")/.."

# 検査する組み合わせ。名前 : 引数 : 警告を落とすか。
combos=(
  "既定                 :               :strict"
  "機能なし             :--no-default-features:lax"
  "ウィンドウのみ       :--no-default-features --features window:lax"
  "whisper のみ         :--no-default-features --features whisper:lax"
  "埋め込み（合成音）   :--features embed-reference:strict"
)

# 本番音源があるときだけ、本番の埋め込みも見る。
# 無いときに回すと include_bytes! が落ちるだけで、確かめたい事とは関係ない。
if ls assets/*.wav >/dev/null 2>&1; then
  combos+=("埋め込み（本番）     :--features embed:strict")
fi

fail=0
printf '%-22s %-8s %s\n' "組み合わせ" "結果" "警告"
printf '%s\n' "--------------------------------------------------------"

for entry in "${combos[@]}"; do
  name="${entry%%:*}"
  rest="${entry#*:}"
  args="${rest%:*}"
  mode="${rest##*:}"
  # shellcheck disable=SC2086
  out="$(cargo check --all-targets $args 2>&1)"
  code=$?
  warn="$(printf '%s' "$out" | grep -c '^warning:')"

  if [ "$code" -ne 0 ]; then
    printf '%-22s %-8s %s\n' "$name" "NG" "-"
    printf '%s\n' "$out" | grep -E '^error' -A4 | head -20
    fail=1
  elif [ "$warn" -ne 0 ] && [ "$mode" = strict ]; then
    printf '%-22s %-8s %s\n' "$name" "警告" "$warn"
    printf '%s\n' "$out" | grep -E '^warning:' -A3 | head -20
    fail=1
  elif [ "$warn" -ne 0 ]; then
    printf '%-22s %-8s %s\n' "$name" "OK" "${warn}（構成上の死にコード）"
  else
    printf '%-22s %-8s %s\n' "$name" "OK" "0"
  fi
done

printf '%s\n' "--------------------------------------------------------"

if cargo test --all-targets >/tmp/donna-check-test.log 2>&1; then
  printf '%-22s %s\n' "テスト" "$(grep -h '^test result' /tmp/donna-check-test.log | head -1)"
else
  printf '%-22s %s\n' "テスト" "NG"
  grep -E 'FAILED|panicked|assertion' /tmp/donna-check-test.log | head -20
  fail=1
fi

if cargo fmt --check >/dev/null 2>&1; then
  printf '%-22s %s\n' "書式" "差分なし"
else
  printf '%-22s %s\n' "書式" "NG（cargo fmt を実行すること）"
  cargo fmt --check 2>&1 | head -20
  fail=1
fi

printf '%s\n' "--------------------------------------------------------"
if [ "$fail" -eq 0 ]; then
  echo "すべて通った。"
else
  echo "通っていないものがある。上を見ること。" >&2
fi
exit "$fail"
