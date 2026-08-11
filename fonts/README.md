# fonts

`again.ttf` は **Noto Sans JP** から「もう1回」の4文字だけを抜いたサブセット。

ウィンドウに出す文字はこれ1語だけなので、全部入りの CJK フォント（5MB 超）を
抱える理由がない。4グリフに削ると 4KB で済む。

作り直すには（`fonttools` が要る）:

```sh
curl -sL -o vf.ttf \
  https://raw.githubusercontent.com/notofonts/noto-cjk/main/Sans/Variable/TTF/Subset/NotoSansJP-VF.ttf
python3 -m fontTools.varLib.instancer vf.ttf wght=700 -o bold.ttf
python3 -m fontTools.subset bold.ttf --text='もう1回' --name-IDs='*' \
  --no-hinting --desubroutinize --output-file=again.ttf
```

可変フォントのままだと `fontdue` が扱えないので、先に太さを固定する。
太字にしてあるのは、テレビから離れて読むため。

**出す文字を増やすときは `--text` を足してサブセットし直すこと。**
入っていない文字は何も描かれない。

ライセンスは SIL Open Font License 1.1（`LICENSE-NotoSansJP.txt`）。
サブセットしても OFL のままなので、この表示ごと残すこと。
