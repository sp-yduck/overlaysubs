# OverlaySubs

[![CI](https://github.com/sp-yduck/overlaysubs/actions/workflows/ci.yml/badge.svg)](https://github.com/sp-yduck/overlaysubs/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/sp-yduck/overlaysubs?label=release&sort=semver)](https://github.com/sp-yduck/overlaysubs/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Windows x64](https://img.shields.io/badge/platform-Windows%20x64-0078d4.svg)](#動作環境)
[![macOS Apple Silicon](https://img.shields.io/badge/platform-macOS%20Apple%20Silicon-lightgrey.svg)](#動作環境)

PC 上で再生されるあらゆる音声をリアルタイムで文字起こし・翻訳し、字幕としてオーバーレイ表示するデスクトップアプリです。Tauri 2（Rust）製で、UI はビルド不要の素の HTML/JS です。API キーは不要、音声・テキストは端末の外に出ません。

**Built with PLaMo.** 翻訳には Preferred Networks の [PLaMo 3 Translate](https://huggingface.co/pfnet/plamo-3-translate-0.8b-GGUF) を使います。

## 利用者向け（ダウンロード・インストール）

**GitHub アカウントは不要です。** アプリを使うだけなら、公式サイトの手順に従ってダウンロードして実行してください。

- 公式サイト（ダウンロード・インストール・使い方・FAQ・アンインストール・ライセンス）: <https://sp-yduck.github.io/overlaysubs/>
- Windows 10 / 11（x64）: [overlaysubs-windows-x64-setup.exe](https://github.com/sp-yduck/overlaysubs/releases/latest/download/overlaysubs-windows-x64-setup.exe)（通常はこちら）
- Windows（MSI）: [overlaysubs-windows-x64.msi](https://github.com/sp-yduck/overlaysubs/releases/latest/download/overlaysubs-windows-x64.msi)（管理者権限でインストールされます。通常は `.exe` を推奨）
- macOS 13 以降（Apple Silicon）: [overlaysubs-macos-arm64.dmg](https://github.com/sp-yduck/overlaysubs/releases/latest/download/overlaysubs-macos-arm64.dmg)（Intel Mac 版はありません）
- チェックサム: [SHA256SUMS.txt](https://github.com/sp-yduck/overlaysubs/releases/latest/download/SHA256SUMS.txt) / リリース一覧: <https://github.com/sp-yduck/overlaysubs/releases/latest>

インストール手順・警告が出たときの操作・SHA-256 の確認・使い方・FAQ・アンインストールは、すべて公式サイトにまとめています。
[インストール](https://sp-yduck.github.io/overlaysubs/#install) / [警告が表示されたとき](https://sp-yduck.github.io/overlaysubs/#warning) / [使い方](https://sp-yduck.github.io/overlaysubs/#usage) / [FAQ](https://sp-yduck.github.io/overlaysubs/#faq) / [アンインストール](https://sp-yduck.github.io/overlaysubs/#uninstall) / [ライセンスと利用規約](https://sp-yduck.github.io/overlaysubs/#license)

> 配布ビルドは**未署名・未公証**です（Windows はコード署名なし、macOS は Developer ID 署名・公証なしの **ad-hoc 署名のみ**）。初回起動時に SmartScreen / Gatekeeper の警告が表示されますが、アプリ自体に問題があるという意味ではありません。回避手順は公式サイトの[警告が表示されたとき](https://sp-yduck.github.io/overlaysubs/#warning)を参照してください（Gatekeeper / SmartScreen をシステム全体で無効にしないでください）。

## 目次

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
| GPU | **Vulkan 対応 GPU + 最新の GPU ドライバ** | 内蔵 GPU（Metal） |
| メモリ | 8GB 以上 | 8GB 以上 |
| 空き容量 | モデルのダウンロード分（[モデルとティア](#モデルとティア)を参照） | 同左 |

- Windows 版は **Vulkan 対応の GPU と最新ドライバ**が必要です（公式ビルドは `--features vulkan`）。ドライバが古いと GPU が使われず、実用的な速度が出ません。
- Intel Mac 版はありません。32bit Windows、Windows on ARM 版もありません。
- 配布は **GitHub Releases のみ**です（自動アップデート機能はありません）。

## 仕組み

```
WASAPI loopback / マイク ─▶ 16kHz mono ─▶ Silero VAD による発話区切り ─▶ whisper.cpp ─▶ PLaMo 3 Translate (llama.cpp) ─▶ オーバーレイ
```

- `crates/core` はエンジン非依存のロジックです。ティア選択、モデル定義、PFML3 プロンプト、発話区切り、セッション、文単位の翻訳、SRT 出力を持ちます。`cargo test -p overlaysubs-core` で検証できます。
- `src-tauri` は Tauri アプリ本体です。音声キャプチャ、whisper.cpp / llama.cpp アダプタ、モデルのダウンロード、トレイ、ホットキー、設定の保存を担当します。
- `ui` は設定画面とオーバーレイです。ビルド不要の素の HTML/JS/CSS で、`ui/i18n.js` に日本語と英語の文言があります。
- 翻訳エンジンと音声認識は `session::Translator` / `session::Asr` トレイトの裏にあり、差し替えられます。
- `models.json` がモデル定義（URL、サイズ、SHA-256、ティア）です。URL は Hugging Face のコミットに固定しています（`main` だと上流の再アップロードでハッシュが合わなくなるため）。設定フォルダに `models.json` を置くと、アプリを更新せずに上書きできます。
- `licenses/` に OverlaySubs 利用規約と PLaMo コミュニティライセンス（日英）、`THIRD_PARTY_NOTICES.md` に同梱ライブラリのライセンスを置いています。

## 機能

- 初回起動で OverlaySubs 利用規約（PLaMo コミュニティライセンスを別紙として含む）に同意します。同意は設定フォルダの `terms-accepted` に保存し、規約の版が変わるまで再表示しません。設定画面の「PC の性能を調べる」でハードウェアを検出し、ティア（High / Mid / Low / CPU）に応じたモデルを自動選択します。
- モデルは Hugging Face から自動ダウンロードします。中断しても再開でき、SHA-256 で検証します。アカウントやトークンは不要です。
- 任意の速度チェック（ベンチマーク）を実行でき、リアルタイムに間に合わなければ1段下げたモデルに切り替えます。
- 音声入力は Windows の WASAPI loopback（システム音声）とマイク。macOS ではループバック入力デバイスをマイクとして選びます。
- 入力言語の自動判定または固定。訳文言語と「入力が訳文と同じ言語のとき」の言語を選べるので、英⇔日・韓⇔日を同時に扱えます。
- 直前の発話を文脈として渡し、文単位で訳します。用語集、話題（翻訳のヒント）、日本語の敬体／常体を指定できます。
- 暫定結果は薄く表示し、確定訳はストリーミングで上書きします。
- 最前面・半透明のオーバーレイ（クリック透過は設定でオンにできます）。原文併記、文字サイズ、フォント、背景透過度、行数、表示時間、「Built with PLaMo」表記の有無を変更できます。
- 履歴をテキスト／SRT で保存できます。
- トレイ常駐、グローバルホットキー（既定 `CmdOrCtrl+Shift+T`。Windows は `Ctrl+Shift+T`、macOS は `Cmd+Shift+T`）で開始・停止。UI は日本語・英語。
- 音声・文字起こし・翻訳データは外部に送信しません。通信はモデルのダウンロードだけです。テレメトリはありません。モデルをダウンロードした後はオフラインでも動作します。

## モデルとティア

ティアは初回セットアップで「PC の性能を調べる」を実行したときに決まり、そのティアに応じたモデルが自動で選択されます。設定画面では使用するモデル（Whisper / PLaMo / VAD）を個別に変更できます。

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

必要なもの: Rust stable（edition 2021）。Windows は MSVC + Windows SDK、CMake、LLVM / libclang（bindgen 用）、Vulkan SDK、WebView2。macOS は Xcode Command Line Tools。Node.js は Tauri CLI を `npx` で動かすためだけに使います（アプリ本体には不要）。

```bash
# Windows（Vulkan。公式ビルドと同じ）
npx --yes @tauri-apps/cli@2.12.4 build --features vulkan

# macOS（Metal）
npx --yes @tauri-apps/cli@2.12.4 build --features metal

# NVIDIA 向け
npx --yes @tauri-apps/cli@2.12.4 build --features cuda
```

`cargo install tauri-cli --version "^2" --locked` をして `cargo tauri build` を使っても構いません。CI はビルド時間を短縮するため、プリビルド済みの `npx @tauri-apps/cli@2.12.4` を使っています。

whisper.cpp と llama.cpp はどちらも ggml を含むため、同じプロセスに静的リンクするとシンボルが衝突します。llama.cpp は共有ライブラリとしてリンクし（`llama-cpp-2` の `dynamic-link`）、インストーラに同梱しています。

- Windows: `src-tauri/libs/*.dll` を `bundle.resources` で同梱します。`src-tauri/libs/` は `.gitignore` 対象なので、バンドル前に llama.cpp のビルド成果物（`target/release/build/llama-cpp-sys-2-*/out/bin/*.dll`）を `src-tauri/libs/` にコピーしてください。CI の手順は `.github/workflows/release.yml` にあります。
- macOS: `beforeBundleCommand` が `scripts/stage-dylibs.sh` を実行し、`target/frameworks` に dylib を集めて `bundle.macOS.files` で同梱します。
- `THIRD_PARTY_NOTICES.md` もバンドルに同梱されます（`src-tauri/tauri.windows.conf.json` / `tauri.macos.conf.json`）。

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

- [CONTRIBUTING.md](CONTRIBUTING.md) — 開発環境、テスト、PR の流れ
- [SECURITY.md](SECURITY.md) — 脆弱性の報告方法（非公開で受け付けます）
- [CHANGELOG.md](CHANGELOG.md) — 変更履歴
- [RELEASING.md](RELEASING.md) — リリース手順（管理者向け）
- [docs/index.html](docs/index.html) — 利用者向けドキュメント（GitHub Pages のソース。外部 CDN・フォント・JS は使わず単体で完結）
- Issue テンプレート: [バグ報告](.github/ISSUE_TEMPLATE/bug_report.yml) / [機能要望](.github/ISSUE_TEMPLATE/feature_request.yml)

## ライセンス

- **アプリのコードは MIT ライセンスです**（[LICENSE](LICENSE)）。
- アプリの利用条件は [OverlaySubs 利用規約](licenses/overlaysubs-terms.ja.txt)（[English](licenses/overlaysubs-terms.en.txt)）です。規約は [PLaMo コミュニティライセンス](https://plamo.preferredai.jp/info/plamo-community-license) を別紙として含み、初回起動時にまとめて同意を求めます。全文を [`licenses/`](licenses/) に同梱し、アプリの「情報」からも参照できます。
  - 個人・非商用の利用は無償です。
  - 商用利用には事前登録が必要です。また、本人または関連会社の年間売上が10億円を超える組織が業務で使う場合は、Preferred Networks との別途契約が必要です。
  - 字幕などの出力を公開・配布する場合は、PLaMo による出力である旨を明示してください。保存したテキスト/SRT には表記を入れないので、公開時はご自身で明記してください。字幕オーバーレイには既定で `Built with PLaMo` を表示します（設定でオフにできます）。
- 配信や動画の字幕を公開する場合は、元コンテンツの著作権にも注意してください。
- Whisper モデル（MIT）と Silero VAD（MIT）は、それぞれのライセンスに従います。
- 同梱している whisper.cpp / llama.cpp / ggml などのサードパーティライセンスは [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) にまとめています。
