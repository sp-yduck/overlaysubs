# floatsubs

PC上で再生されるあらゆる音声をリアルタイムで文字起こし・翻訳し、字幕としてオーバーレイ表示するデスクトップアプリです。
**インストールして起動するだけ。APIキー不要・完全無料・完全オフライン。**

**Built with PLaMo.** 翻訳には Preferred Networks の [PLaMo 3 Translate](https://huggingface.co/pfnet/plamo-3-translate-0.8b-GGUF) を使います。

## 仕組み

```
WASAPI loopback / マイク ─▶ 16kHz mono ─▶ エネルギー区切り ─▶ whisper.cpp (+ Silero VAD) ─▶ PLaMo 3 Translate (llama.cpp) ─▶ オーバーレイ
```

- `crates/core` はエンジン非依存のロジックです。ティア選択、モデル定義、PFML3 プロンプト、発話区切り、セッション、SRT 出力を持ちます。`cargo test -p floatsubs-core` で検証できます。
- `src-tauri` は Tauri アプリ本体です。音声キャプチャ、whisper.cpp / llama.cpp アダプタ、ダウンロード、トレイ、ホットキーを担当します。
- `ui` は設定画面とオーバーレイです。ビルド不要の素の HTML/JS です。
- 翻訳エンジンと音声認識は `session::Translator` / `session::Asr` トレイトの裏にあり、差し替えられます。
- `models.json` がモデル定義（URL、サイズ、SHA-256、ティア）です。設定フォルダに `models.json` を置くと、アプリを更新せずに上書きできます。

## 機能

- 初回起動でライセンス同意。その後ハードウェアを検出し、ティア（High / Mid / Low / CPU）からモデルを自動選択します。
- モデルは Hugging Face から自動ダウンロードします。中断しても再開でき、SHA-256 で検証します。アカウントやトークンは不要です。
- ベンチマークを実行し、リアルタイムに間に合わなければ1段下げます。
- 入力言語の自動判定または固定。訳文言語と「入力が訳文と同じ言語のとき」の言語を選べるので、英⇔日・韓⇔日を同時に扱えます。
- 直前3発話を文脈として渡します。用語集と日本語の敬体／常体を指定できます。
- 暫定結果は薄く表示し、確定訳はストリーミングで上書きします。
- 最前面・透過・クリック透過のオーバーレイ。原文併記、文字サイズ、フォント、背景透過度、行数、表示時間を変更できます。
- 履歴をテキスト／SRTで保存できます。
- トレイ常駐、グローバルホットキー（既定 `Ctrl+Shift+T` / `Cmd+Shift+T`）で開始・停止。UI は日本語・英語。
- 音声・文字起こし・翻訳データは外部に送信しません。通信はモデルのダウンロードだけです。テレメトリはありません。

## ティア

| ティア | 判定 | Whisper | PLaMo 3 Translate |
|---|---|---|---|
| High | VRAM 12GB以上 / Apple Silicon RAM 32GB以上 | large-v3-turbo | 8B Q4_K_M |
| Mid | VRAM 8GB / Apple Silicon RAM 16GB | large-v3-turbo Q5_0 | 2B Q4_K_M |
| Low | VRAM 4〜6GB / Apple Silicon RAM 8GB | small | 2B Q4_K_M |
| CPU | GPUなし | base | 0.8B Q4_K_M |

Mid で 8B ではなく 2B を選んでいるのは、8GB の VRAM に Whisper と 8B Q4_K_M（5.3GB）を同居させるとゲームと併用する余裕がないためです。設定画面で 8B Q3_K_M や 31B に手動で変更できます。

## ビルド

```bash
cargo install tauri-cli --version "^2" --locked
cd src-tauri && cargo tauri build --features vulkan   # Windows
cd src-tauri && cargo tauri build --features metal    # macOS
```

NVIDIA 向けには `--features cuda` を使います。whisper.cpp と llama.cpp はどちらも ggml を含むため、llama.cpp は共有ライブラリとしてリンクし、インストーラに同梱します。

## 現状の制限

- macOS のシステム音声キャプチャ（ScreenCaptureKit）は未実装です。macOS ではループバック入力デバイスを「マイク」として選んでください。
- 特定アプリだけのキャプチャ、システム音声とマイクの同時処理は未実装です。
- 署名・公証済み .dmg とインストーラの動作は CI 上でまだ確認していません。

## ライセンス

- **アプリのコードは MIT ライセンスです**（[LICENSE](LICENSE)）。
- **PLaMo モデルは [PLaMo コミュニティライセンス](https://plamo.preferredai.jp/info/plamo-community-license) に従います。** 全文を [`licenses/`](licenses/) に同梱し、アプリ内からも参照できます。
  - 個人・非商用の利用は無償です。
  - 商用利用には事前登録が必要です。また、本人または関連会社の年間売上が10億円を超える組織が業務で使う場合は、Preferred Networks との別途契約が必要です。
  - 字幕などの出力を公開・配布する場合は、PLaMo による出力である旨を明示してください。テキスト出力には `Built with PLaMo` の表記が入ります。
- 配信や動画の字幕を公開する場合は、元コンテンツの著作権にも注意してください。
- Whisper モデル（MIT）と Silero VAD（MIT）は、それぞれのライセンスに従います。
