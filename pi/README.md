# pi — ラズパイ側の設定

**バイナリに入れられなかったもの。** アプリは1つのバイナリで完結するが、
この2つは OS の側の設定なので外に出るしかない。**手で置くと Git の外に
出て、次に組み直したときに再現できない**ので、ここで持って
`tools/build-pi.sh` に配らせる。

| ここ | 配る先 | 何のため |
| --- | --- | --- |
| `asoundrc` | `~/.asoundrc` | ssh から効く音量つまみを作る |
| `labwc-rc.xml` | `~/.config/labwc/rc.xml` | ウィンドウを全画面にする |

**どちらも実機で検証した実物**で、写しではない。

## asoundrc

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
