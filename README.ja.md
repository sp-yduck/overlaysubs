# OverlaySubs

[English](README.md) · **日本語**

[![CI](https://github.com/sp-yduck/overlaysubs/actions/workflows/ci.yml/badge.svg)](https://github.com/sp-yduck/overlaysubs/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/sp-yduck/overlaysubs?label=release&sort=semver)](https://github.com/sp-yduck/overlaysubs/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Windows x64](https://img.shields.io/badge/platform-Windows%20x64-0078d4.svg)](#動作環境)
[![macOS Apple Silicon](https://img.shields.io/badge/platform-macOS%20Apple%20Silicon-lightgrey.svg)](#動作環境)

PC 上で再生されるあらゆる音声をリアルタイムで文字起こし・翻訳し、字幕としてオーバーレイ表示するデスクトップアプリです。音声とテキストは端末内でローカルに処理されます。API キーは不要で、音声やテキストが外部のサービスに送信されることはありません。

**Built with PLaMo.** 翻訳には Preferred Networks の [PLaMo 3 Translate](https://huggingface.co/pfnet/plamo-3-translate-0.8b-GGUF) を使います。

## スクリーンショット

**設定画面（翻訳）** — 言語・翻訳の調整・最近の字幕。
[![設定画面](docs/images/ui-settings-ja.png)](docs/images/ui-settings-ja.png)

**モデルの準備** — 初回起動時にハードウェアを判定し、ティアに合ったモデルをダウンロードします。
[![モデルの準備画面](docs/images/ui-model-setup-ja.png)](docs/images/ui-model-setup-ja.png)

**字幕オーバーレイ** — 他のアプリの上に重ねて表示されます。クリック透過は設定でオンにできます（背景はイメージ）。
[![字幕オーバーレイ](docs/images/overlay.jpg)](docs/images/overlay.jpg)

## 利用者向け（ダウンロード・インストール）

**GitHub アカウントは不要です。** アプリを使うだけなら、公式サイトの手順に従ってダウンロードして実行してください。

- 公式サイト（ダウンロード・インストール・使い方・FAQ・アンインストール・ライセンス）: <https://sp-yduck.github.io/overlaysubs/ja/>
- Windows 10 / 11（x64）: [overlaysubs-windows-x64-setup.exe](https://github.com/sp-yduck/overlaysubs/releases/latest/download/overlaysubs-windows-x64-setup.exe)（通常はこちら）
- Windows（MSI）: [overlaysubs-windows-x64.msi](https://github.com/sp-yduck/overlaysubs/releases/latest/download/overlaysubs-windows-x64.msi)（管理者権限でインストールされます。通常は `.exe` を推奨）
- macOS 13 以降（Apple Silicon）: [overlaysubs-macos-arm64.dmg](https://github.com/sp-yduck/overlaysubs/releases/latest/download/overlaysubs-macos-arm64.dmg)（Intel Mac 版はありません）
- チェックサム: [SHA256SUMS.txt](https://github.com/sp-yduck/overlaysubs/releases/latest/download/SHA256SUMS.txt) / リリース一覧: <https://github.com/sp-yduck/overlaysubs/releases/latest>

インストール手順・警告が出たときの操作・SHA-256 の確認・使い方・FAQ・アンインストールは、すべて公式サイトにまとめています。
[インストール](https://sp-yduck.github.io/overlaysubs/ja/#install) / [警告が表示されたとき](https://sp-yduck.github.io/overlaysubs/ja/#warning) / [使い方](https://sp-yduck.github.io/overlaysubs/ja/#usage) / [FAQ](https://sp-yduck.github.io/overlaysubs/ja/#faq) / [アンインストール](https://sp-yduck.github.io/overlaysubs/ja/#uninstall) / [ライセンスと利用規約](https://sp-yduck.github.io/overlaysubs/ja/#license)

> 配布ビルドは**未署名・未公証**です（Windows はコード署名なし、macOS は Developer ID 署名・公証なしの **ad-hoc 署名のみ**）。初回起動時に SmartScreen / Gatekeeper の警告が表示されますが、アプリ自体に問題があるという意味ではありません。回避手順は公式サイトの[警告が表示されたとき](https://sp-yduck.github.io/overlaysubs/ja/#warning)を参照してください（Gatekeeper / SmartScreen をシステム全体で無効にしないでください）。

## 目次

- [スクリーンショット](#スクリーンショット)
- [動作環境](#動作環境)
- [仕組み](#仕組み)
- [機能](#機能)
- [モデルとティア](#モデルとティア)
- [ビルド](#ビルド)
- [テスト](#テスト)
- [開発に参加する](#開発に参加する)
- [ライセンス](#ライセンス)

## 動作環境

| | Windows | macOS |
|---|---|---|
| OS | Windows 10 / 11（64bit） | macOS 13 Ventura 以降 |
| CPU | x64（Intel / AMD） | Apple Silicon（M1 以降） |
| GPU | **推奨: Vulkan 対応 GPU + 最新の GPU ドライバ**（なくても CPU で動作） | 内蔵 GPU（Metal） |
| メモリ | 8GB 以上 | 8GB 以上 |
| 空き容量 | モデルのダウンロード分（[モデルとティア](#モデルとティア)を参照） | 同左 |

- Windows 版は、**Vulkan ランタイムを別途インストールしなくても起動します**。公式ビルド（`--features vulkan`）はロード時に `vulkan-1` をリンクするため、インストーラに Khronos の Vulkan ローダー（`vulkan-1.dll`、Apache-2.0）を同梱しています。Vulkan 対応 GPU やドライバがない PC ではローダーがデバイスを見つけられず、CPU で動作します（CPU ティアのモデルを使うため遅くなります）。
- 実用的な速度で動かすには **Vulkan 対応 GPU と最新ドライバを強く推奨します**。ドライバが古いと GPU が使われず、実用的な速度が出ません。
- Intel Mac 版はありません。32bit Windows、Windows on ARM 版もありません。
- 配布は **GitHub Releases のみ**です（自動アップデート機能はありません。新しいリリースがある場合に通知し、リリースページへのリンクを表示するだけです）。

## 仕組み

```
WASAPI loopback（Windows）/ ScreenCaptureKit のシステム音声（macOS）/ マイク ─▶ 16kHz mono ─▶ Silero VAD による発話区切り ─▶ whisper.cpp ─▶ PLaMo 3 Translate (llama.cpp) ─▶ オーバーレイ
```

- `crates/core` はエンジン非依存のロジックです。ティア選択、モデル定義、PFML3 プロンプト、発話区切り、セッション、文単位の翻訳、SRT 出力を持ちます。`cargo test -p overlaysubs-core` で検証できます。
- `src-tauri` は Tauri アプリ本体です。音声キャプチャ、whisper.cpp / llama.cpp アダプタ、モデルのダウンロード、トレイ、ホットキー、設定の保存を担当します。
- `ui` は設定画面とオーバーレイです。ビルド不要の素の HTML/JS/CSS で、`ui/i18n.js` に日本語・英語・中国語（繁体字・簡体字）の文言があります。
- 翻訳エンジンと音声認識は `session::Translator` / `session::Asr` トレイトの裏にあり、差し替えられます。
- `models.json` がモデル定義（URL、サイズ、SHA-256、ティア）です。URL は Hugging Face のコミットに固定しています（`main` だと上流の再アップロードでハッシュが合わなくなるため）。設定フォルダに `models.json` を置くと、アプリを更新せずに上書きできます。
- `licenses/` に OverlaySubs 利用規約（日英）、`THIRD_PARTY_NOTICES.md` に同梱ライブラリのライセンスを置いています。PLaMo コミュニティライセンス（2026年11月9日改定）は同梱せず、公式ページへのリンクのみを掲載します。

## 機能

- 初回起動で OverlaySubs 利用規約に同意します。同意は設定フォルダの `terms-accepted` に保存し、規約の版が変わるまで再表示しません。設定画面の「PC の性能を調べる」でハードウェアを検出し、ティア（High / Mid / Low / CPU）に応じたモデルを自動選択します。検出結果は `hardware.json` にキャッシュし、次回起動時はハードウェアを調べ直さずに表示します（「もう一度調べる」で再検出）。
- モデルは Hugging Face から自動ダウンロードします。中断しても再開でき、SHA-256 で検証します。アカウントやトークンは不要です。
- 任意の速度チェック（ベンチマーク）を実行でき、リアルタイムに間に合わなければ1段下げたモデルに切り替えます。
- 音声入力はシステム音声とマイクです。Windows は WASAPI loopback で、macOS 13 以降は ScreenCaptureKit でシステム音声（すべてのアプリの再生音）を直接取り込みます。BlackHole などのループバック（仮想オーディオ）デバイスは不要です。マイクは Windows・macOS の両方で使えます。macOS で初めてシステム音声を使うときは、システム設定 → プライバシーとセキュリティの「画面収録とシステム音声録音」（macOS 15 以降。macOS 13〜14 は「画面収録」）で OverlaySubs を許可し、アプリを再起動してください。
- 入力言語の自動判定または固定。訳文言語と「入力が訳文と同じ言語のとき」の言語を選べるので、英⇔日・韓⇔日を同時に扱えます。
- 直前の発話を文脈として渡し、文単位で訳します。用語集、話題（翻訳のヒント）、日本語の敬体／常体を指定できます。
- 暫定結果は薄く表示し、確定訳はストリーミングで上書きします。
- 最前面・半透明のオーバーレイ（クリック透過は設定でオンにできます）。オーバーレイと字幕背景の色・不透明度、訳文のフォント・文字サイズ・色・太さ・行間、原文の表示有無・文字サイズ・色、字幕の文字の揃え・縦位置・表示行数、字幕の更新が止まってから消えるまでの時間を変更できます。行がオーバーレイに収まらないときは、右端のスクロールバーかマウスホイールでスクロールできます（ホイールは半行ずつ。クリック透過がオフのとき）。字幕の更新が続いている間は、表示行数で押し出されるまで遡れます。翻訳中だけ表示され、停止すると隠れます。トレイメニューの「Show subtitles」または翻訳開始ボタンの上にあるトグルで手動で表示・非表示を切り替えることもできます。翻訳を停止中に「字幕の表示」を開くと、オーバーレイを表示してサンプル字幕を出し、見た目を確認しながら調整できます。
- 履歴をテキスト／SRT で保存できます。
- トレイ常駐、グローバルホットキー（既定 `CmdOrCtrl+Shift+T`。Windows は `Ctrl+Shift+T`、macOS は `Cmd+Shift+T`）で開始・停止。UI は英語・日本語・中国語（繁体字・簡体字）で、初回起動時に OS の言語設定から選びます（どれにも当てはまらなければ英語）。利用規約は日本語と英語のみで、中国語の UI では英語版を表示します。
- 起動時に GitHub Releases を一度だけ匿名で確認し、新しいバージョンがあれば設定画面に通知バナーを表示します。最新バージョンと現在のバージョンに加えて、「リリースページを開く」と「後で」のボタンが表示されます。ダウンロード・インストールは自動では行わず、オフラインや API エラーなどで確認できない場合は何も表示しません。「後で」で閉じると、次回起動時までは再表示されません。
- 音声・文字起こし・翻訳データは外部に送信しません。テレメトリはありません。通信はモデルのダウンロードと、起動時に新しいリリースがあるかを確認する GitHub API への匿名のリクエスト 1 回だけです（アカウント・トークン・個人データは送信しません。失敗しても何も起きません）。モデルをダウンロードした後はオフラインでも動作します。

## モデルとティア

ティアは初回セットアップで「PC の性能を調べる」を実行したときに決まり、そのティアに応じたモデルが自動で選択されます。設定画面では使用するモデル（Whisper / PLaMo / VAD）を個別に変更できます。検出結果はキャッシュされるため、グラフィックボードを交換した場合などは「もう一度調べる」で再検出してください。

3 つのモデルにはそれぞれ**実行先（Runs on）**の選択肢があり、バックエンドが対応していれば設定画面で **自動 / GPU / CPU** から選べます。Whisper と PLaMo はどちらでも動きますが、VAD は上流の仕様で CPU 専用です（whisper.cpp が GPU 版 VAD を強制的に無効にしています）。「自動」は GPU が検出できたときに GPU、それ以外は CPU を使います。GPU での読み込みに失敗した場合は、そのモデルを CPU で読み込み直します。モデルや実行先の変更は、次に翻訳を開始したときから反映されます。

| ティア | 判定 | Whisper | PLaMo 3 Translate | 初回ダウンロード量 |
|---|---|---|---|---|
| High | VRAM 12GB以上 / Apple Silicon RAM 32GB以上 | large-v3-turbo | 8B Q4_K_M | 約 6.9GB |
| Mid | VRAM 8GB / Apple Silicon RAM 16GB | large-v3-turbo Q5_0 | 2B Q4_K_M | 約 2.4GB |
| Low | VRAM 4〜6GB / Apple Silicon RAM 8GB | small | 2B Q4_K_M | 約 2.3GB |
| CPU | GPU なし、または VRAM 4GB 未満 | base | 0.8B Q4_K_M | 約 0.7GB |

- サイズは `models.json` の宣言サイズの合計（Silero VAD の約 0.9MB を含む）です。空き容量は目安として High 10GB / Mid・Low 5GB / CPU 2GB 以上を確保してください。
- Mid で 8B ではなく 2B を選んでいるのは、8GB の VRAM に Whisper と 8B Q4_K_M（5.3GB）を同居させるとゲームと併用する余裕がないためです。設定画面で 8B Q3_K_M や 31B に手動で変更できます。
- 速度チェック（ベンチマーク）は任意です。遅い場合は1段下げたモデルを提案し、そのまま切り替えられます。

## ビルド

必要なもの: Rust stable（edition 2021）。Windows は MSVC + Windows SDK、CMake、LLVM / libclang（bindgen 用）、WebView2。**Vulkan SDK が必要なのは `--features vulkan`（公式の GPU ビルド）だけ**で、同梱する Vulkan ローダーをバンドル前に `src-tauri/libs/` へコピーします（後述）。GPU 機能を付けないビルドは CPU 専用で、Vulkan SDK は不要です。macOS は Xcode 26（または macOS 26 SDK を含む Command Line Tools）が必要です。ScreenCaptureKit の Rust バインディングがビルド時に小さな Swift ブリッジをコンパイルし、その依存クレート（apple-metal）が macOS 26 SDK の Metal API を参照するためです。リリース CI は `macos-15` ランナー上の Xcode 26 を選択してビルドします。Node.js は Tauri CLI を `npx` で動かすためだけに使います（アプリ本体には不要）。

```bash
# Windows（Vulkan。公式ビルドと同じ）
npx --yes @tauri-apps/cli@2.12.4 build --features vulkan

# Windows（CPU 専用。Vulkan SDK は不要）
npx --yes @tauri-apps/cli@2.12.4 build

# macOS（Metal）
npx --yes @tauri-apps/cli@2.12.4 build --features metal

# NVIDIA 向け
npx --yes @tauri-apps/cli@2.12.4 build --features cuda
```

`cargo install tauri-cli --version "^2" --locked` をして `cargo tauri build` を使っても構いません。CI はビルド時間を短縮するため、プリビルド済みの `npx @tauri-apps/cli@2.12.4` を使っています。

whisper.cpp と llama.cpp はどちらも ggml を含むため、同じプロセスに静的リンクするとシンボルが衝突します。llama.cpp は共有ライブラリとしてリンクし（`llama-cpp-2` の `dynamic-link`）、インストーラに同梱しています。

- Windows: `src-tauri/libs/*.dll` を `bundle.resources` で同梱します。`src-tauri/libs/` は `.gitignore` 対象なので、バンドル前に llama.cpp のビルド成果物（`target/release/build/llama-cpp-sys-2-*/out/bin/*.dll`）を `src-tauri/libs/` にコピーしてください。`--features vulkan` の場合は Khronos の Vulkan ローダー（`"%VULKAN_SDK%\Bin\vulkan-1.dll"`）もコピーします。`overlaysubs.exe` はロード時に `vulkan-1.dll` を必要とするため、Vulkan 機能付きでビルドしたインストーラにローダーが入っていないと、Vulkan ランタイムのない PC では起動しません。CI の手順は `.github/workflows/release.yml` にあります。
- macOS: `beforeBundleCommand` が `scripts/stage-dylibs.sh` を実行し、`target/frameworks` に dylib を集めて `bundle.macOS.files` で同梱します。
- `THIRD_PARTY_NOTICES.md` もバンドルに同梱されます（`src-tauri/tauri.windows.conf.json` / `tauri.macos.conf.json`）。
- macOS: `src-tauri/Info.plist` はバンドルの生成される Info.plist にマージされます（`bundle.macOS.infoPlist`）。マイクと画面収録／システム音声の利用目的（`NSMicrophoneUsageDescription` / `NSScreenCaptureUsageDescription`）を宣言しています。

## テスト

ネットワーク不要のコアテストは、プルリクエストで CI（`.github/workflows/ci.yml`）が実行します。

```bash
cargo test -p overlaysubs-core   # ティア選択・モデル定義・PFML3・発話区切り・セッション・SRT
cargo test -p overlaysubs        # src-tauri のユニットテスト
```

実モデルを使うエンドツーエンドのテストは既定で `ignore` されています（16kHz モノラルの WAV が必要、約 0.7GB のモデルをダウンロードします）。

```bash
OVERLAYSUBS_WAV=path/to/16k-mono.wav cargo test -p overlaysubs -- --ignored
```

タグ `v*` を push するとリリース用ワークフローがバンドルをビルドして GitHub Releases に公開します。手順は [RELEASING.md](RELEASING.md) を参照してください。

## 開発に参加する

- [CONTRIBUTING.ja.md](CONTRIBUTING.ja.md) — 開発環境、テスト、PR の流れ
- [SECURITY.md](SECURITY.md) — 脆弱性の報告方法（非公開で受け付けます）
- [RELEASING.md](RELEASING.md) — リリース手順（管理者向け）
- [docs/index.html](docs/index.html) — 利用者向けドキュメント（GitHub Pages のソース。英語 docs/index.html / 日本語 docs/ja/index.html、共通 CSS。英語ページは外部 CDN・フォント・JS なしで完結）
- Issue テンプレート: [バグ報告](.github/ISSUE_TEMPLATE/bug_report.yml) / [機能要望](.github/ISSUE_TEMPLATE/feature_request.yml)

## ライセンス

- **アプリのコードは MIT ライセンスです**（[LICENSE](LICENSE)）。
- アプリの利用条件は [OverlaySubs 利用規約](licenses/overlaysubs-terms.ja.txt)（[English](licenses/overlaysubs-terms.en.txt)）です。初回起動時に同意を求め、アプリの「情報」からも参照できます。
- 翻訳モデル PLaMo 3 Translate の利用条件は、Preferred Networks が 2026年11月9日に改定した [PLaMo コミュニティライセンス](https://plamo.preferredai.jp/info/plamo-community-license-ja) に従います。ライセンス本文は同梱せず、アプリとドキュメントから公式ページへのリンクのみを掲載します。
- 個人利用・学術利用は無償です。企業その他の組織内での利用を含む商業目的の利用には、Preferred Networks への事前登録が必要です。直近事業年度の収入または売上が10億円以下の場合は無償で、これを超える場合は別途のライセンス契約が必要です。
- 改定後のライセンスでは、翻訳した字幕の公開にクレジット表示は必須ではありません。書籍・論文その他これらに類する長文の公表物では、PLaMo を翻訳に利用した旨を明示してください。字幕データをデータセット等として配布する場合は、そのデータで学習したモデルに本ライセンスが適用される旨を明示してください。
- 配信や動画の字幕を公開する場合は、元コンテンツの著作権にも注意してください。
- Whisper モデル（MIT）と Silero VAD（MIT）は、それぞれのライセンスに従います。
- 同梱している whisper.cpp / llama.cpp / ggml（MIT）と Windows 版の Vulkan ローダー `vulkan-1.dll`（Apache-2.0）などのサードパーティライセンスは [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) にまとめています。
