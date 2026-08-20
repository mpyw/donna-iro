# pi — ラズパイ側の設定

**バイナリに入れられなかったもの。** アプリは1つのバイナリで完結するが、
この2つは OS の側の設定なので外に出るしかない。**手で置くと Git の外に
出て、次に組み直したときに再現できない**ので、ここで持って
`tools/build-pi.sh` に配らせる。

| ここ | 配る先 | 何のため |
| --- | --- | --- |
| `asound.conf` | `/etc/asound.conf` | ssh から効く音量つまみを作る |
| `labwc-rc.xml` | `~/.config/labwc/rc.xml` | ウィンドウを全画面にする |

**どちらも実機で検証した実物**で、写しではない。

## asound.conf

**`~/.asoundrc` に置いてはいけない。** デスクトップの音量プラグインがあの
ファイルの所有権を主張してくる。実際に、パネルから出力を切り替えたあとで
消えていた（labwc の `rc.xml` は残っていたので、再起動のせいではない）。
`/etc/asound.conf` なら触られない。

**ここだけ `sudo` が要るので、`tools/build-pi.sh` は書き込まない。** 違って
いれば `/tmp` に置いて、打つべき一行を出す。パスワードを聞かれる経路を
スクリプトに隠すと、無人で回したときに黙って止まる。


**vc4hdmi の `PCM Playback Volume` は値を受け取るだけで減衰しない。**
`amixer -c 0 sset PCM 5%` が `[-48.40dB]` を表示しても音量は変わらない。
そこで `softvol` をソフト側に挟んで、実際に効くつまみを作る。

```sh
amixer -c 0 sset Softvol 15%   # これは効く
amixer -c 0 sset PCM 15%       # これは効かない（触らないこと）
```

**PipeWire は動いているが、当てにできない。** `/etc/alsa/conf.d/` も
`asound.conf` も無いので ALSA の `default` は PipeWire を経由せず、
cpal（= donna-iro）もハードを直接叩く。だから `wpctl` で下げても効かない。

`asym` の `capture.pcm` を書いてあるのは、無いと
「capture slave is not defined」と `dmix` の警告が大量に出るため。
カードは番号ではなく名前（`CARD=vc4hdmi0` / `CARD=V15AF`）で指す。
**番号は差し替えでずれる。**

`Softvol` のコントロールは**一度その PCM を使うまで生えない。** 先に
`aplay -D default -f cd -t raw -d 1 /dev/zero`（全部ゼロ = 無音）を流せば、
音を出さずに生やせる。

## labwc-rc.xml

理由は本体の README「テレビで全画面にする」を見ること。要点だけ:

- **minifb にフルスクリーンの API が無い**のでコンポジタ側で当てる
- **`title="*"` なのは絞れないから。** minifb は map の後にタイトルを付け、
  `set_app_id` を呼ばないので、ルールを当てる瞬間このウィンドウは名前を持たない
- labwc は `-m`（merge-config）で起動しているので、これは
  `/etc/xdg/labwc/rc.xml` の既定に**足される**（置き換えではない）
- 直したら `killall -HUP labwc`（ログアウト不要）
