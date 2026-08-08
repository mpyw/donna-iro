#!/usr/bin/env bash
# macOS の .app を作る。
#
#   tools/bundle-mac.sh
#
# Metal は macOS で既定有効なので、指定は要らない。
#
# 音源とモデルは埋め込む。.app は CWD が / になるため、相対パスの
# assets/ や models/ を読めない。埋め込まないと動かない。

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

NAME="donna-iro"
APP="target/$NAME.app"
BUNDLE_ID="com.mpyw.donna-iro"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"

# --- 埋め込む実体があるか先に見る。ビルドしてから失敗すると時間の無駄 ---
missing=()
for stem in intro question tail tail-lead bridge interlude all \
  red blue yellow green yellowgreen white black pink orange purple brown lightblue; do
  [ -f "assets/$stem.wav" ] || missing+=("assets/$stem.wav")
done
[ -f models/ggml-base.bin ] || missing+=("models/ggml-base.bin")

if [ ${#missing[@]} -gt 0 ]; then
  echo ".app にするには音源とモデルを埋め込む必要があります（CWD が / になるため）。" >&2
  echo "足りないもの: ${#missing[@]} 件" >&2
  printf '  %s\n' "${missing[@]:0:5}" >&2
  [ ${#missing[@]} -gt 5 ] && echo "  ..." >&2
  echo >&2
  echo "つくよみちゃんの音源がまだなら、確認用の合成音で代用できます:" >&2
  echo "  for f in assets/reference/*.wav; do cp \"\$f\" assets/; done" >&2
  echo "モデルは:" >&2
  echo "  tools/fetch-model.sh" >&2
  exit 1
fi

echo "ビルド中（Metal は macOS で既定有効。whisper.cpp を組み直すので数分かかります）"
cargo build --release --features embed

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "target/release/$NAME" "$APP/Contents/MacOS/$NAME"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>$NAME</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleName</key><string>どんないろがすき</string>
  <key>CFBundleDisplayName</key><string>どんないろがすき</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <!-- これが無いと GUI アプリはマイクを取れない。文言はダイアログに出る。 -->
  <key>NSMicrophoneUsageDescription</key>
  <string>「どんないろがすき？」の答えを聞き取るためにマイクを使います。</string>
</dict>
</plist>
PLIST

# アドホック署名。配布用ではないが、これが無いとマイクの許可が
# 毎回リセットされたり、そもそも起動を拒まれたりする。
codesign --force --deep --sign - "$APP"

echo
echo "完成: $ROOT/$APP ($(du -sh "$APP" | cut -f1))"
echo "  open $APP"
echo
echo "初回はマイクの許可を聞かれます。拒否すると認識が動きません。"
echo "あとから変えるには システム設定 → プライバシーとセキュリティ → マイク。"
