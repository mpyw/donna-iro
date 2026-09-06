#!/usr/bin/env bash
# テレビのリモコンが何をどう届けるかを見る。**ラズパイをテレビに繋いでから。**
#
#   tools/cec-probe.sh
#
# CEC は HDMI の規格そのものなので、変換器は要らない（PC で試すときだけ要る）。
# ただし**何が届くかは機種で変わる**ので、実装の前にここを見る。

echo "── 1. CEC のデバイスが出ているか ──"
if ls /dev/cec* 2>/dev/null; then
  echo "  ある。"
else
  echo "  無い。vc4 のドライバが CEC を出していない。"
  echo "  /boot/firmware/config.txt の dtoverlay=vc4-kms-v3d を確かめること。"
  exit 1
fi

echo
echo "── 2. 道具 ──"
for t in cec-ctl cec-client; do
  command -v $t >/dev/null && echo "  $t あり" || echo "  $t 無し（$t は ${t/cec-ctl/v4l-utils}${t/cec-client/cec-utils} に入っている）"
done
command -v cec-ctl >/dev/null || exit 1

echo
echo "── 3. 相手が見えているか ──"
cec-ctl -d0 --playback -S 2>&1 | head -20

echo
echo "── 4. リモコンを押してみる（20秒） ──"
echo "  テレビのリモコンで、決定・色ボタン・数字などを順に押すこと。"
echo "  何も出なければ、テレビ側の CEC 設定が切れている可能性がある。"
echo "  （Bravia Sync / Anynet+ / Viera Link / REGZA リンク など名前が機種で違う）"
timeout 20 cec-ctl -d0 --monitor 2>&1 | grep -iE 'user-control|key|press' || echo "  何も届かなかった。"

echo
echo "── 5. 入力デバイスとして生えていないか ──"
echo "  CEC が rc-core 経由で普通のキーとして来る機種もある。"
ls /dev/input/event* 2>/dev/null
command -v ir-keytable >/dev/null && ir-keytable 2>&1 | head -20 || echo "  ir-keytable 無し（v4l-utils）"

echo
echo "── まとめ ──"
echo "  4 で何か出た → CEC を直接読む送り手を書く"
echo "  5 にキーが生えている → evdev を読むほうが簡単。どちらも Channel に流すだけ"
