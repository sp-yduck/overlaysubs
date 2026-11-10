# OverlaySubs へのコントリビューション

[English](CONTRIBUTING.md) · **日本語**

> [!NOTE]
> この日本語版は補助的な翻訳です。正文は英語版 [CONTRIBUTING.md](CONTRIBUTING.md) です。内容が食い違う場合は英語版に従ってください。

OverlaySubs へのコントリビューションを歓迎します。バグ報告・機能要望・翻訳の改善・ドキュメントの修正など、コード以外の貢献も歓迎です。

このプロジェクトは [Contributor Covenant 2.1](CODE_OF_CONDUCT.md) を採用しています。参加する前に目を通してください。

- 大きな変更は、着手前に Issue で方針を相談してください（実装方針のすり合わせと、作業の重複を避けるため）。
- 脆弱性を見つけた場合は、**公開 Issue に書かずに** [SECURITY.md](SECURITY.md) の手順で報告してください。
- コントリビューションは [MIT ライセンス](LICENSE) の下で提供されます。CLA（Contributor License Agreement）への署名は不要です。

## 言語

このリポジトリの基本言語は英語です。Issue・Pull Request・コミットメッセージは英語で書いてください。

日本語は、利用者向けの内容（`README.ja.md`、`docs/` 内の日本語ページ、日本語の利用規約、`ui/i18n.js` の日本語 UI 文言）のサブ言語として維持します。利用者向けの文言を変更するときは、英語を先に更新し、日本語訳もあわせて更新してください。

機能上のデータや法的文書は翻訳しないでください。利用規約（`licenses/`）、`crates/core` の幻覚（hallucination）判定用の定型句と言語別の句読点処理、モデルやファイルの識別子はそのままにします。

## 開発環境

### 共通

- **Rust stable**（edition 2021）。`rustup` で導入してください。
- **Git**。Windows では `git config --global core.longpaths true` を設定してください（llama.cpp のサブモジュールのパスが Windows の MAX_PATH を超えるため）。
- **Node.js 20 以降**。アプリ本体には不要で、Tauri CLI を `npx` 経由で動かすためだけに使います。
- 初回ビルドでは whisper.cpp と llama.cpp（ggml）を C/C++ からビルドするため、**10〜20 分**かかり、`target/` が数 GB になります。2 回目以降は Incremental ビルドで数秒〜数十秒です。

### Windows

- **Visual Studio 2022 Build Tools** の「C++ によるデスクトップ開発」ワークロード（MSVC + Windows SDK）。
- **CMake** と **LLVM / libclang**（whisper-rs と llama-cpp-sys が bindgen でバインディングを生成するため）。
- **WebView2 Runtime**（Windows 10/11 には通常プリインストール済み）。
- **Vulkan SDK** — 必須なのは Vulkan（GPU）ビルド（`--features vulkan`）だけです。CI は LunarG のインストーラで `1.4.313.2` を導入します。`VULKAN_SDK` を設定し、`%VULKAN_SDK%\Bin` を `PATH` に追加してください。Windows のリリースバンドルには Khronos の Vulkan ローダーも含める必要があるため、バンドル前に `%VULKAN_SDK%\Bin\vulkan-1.dll` を llama.cpp の DLL と一緒に `src-tauri/libs/` へコピーしてください（CI も同じことをしています）。`overlaysubs.exe` はロード時に `vulkan-1.dll` を必要とするため、これがないと Vulkan ランタイムのない PC では起動しません。GPU 機能を付けないビルドは CPU 専用で、Vulkan SDK は不要です。
- パスが長すぎるエラーが出る場合は、`CARGO_TARGET_DIR` を短いパスに設定してください（CI は `C:\t` を使用）。

```powershell
$env:CARGO_TARGET_DIR = "C:\t"
```

### macOS

- **Xcode Command Line Tools**（`xcode-select --install`）。
- Apple Silicon（M1 以降）と macOS 13 以降。ビルドは `--features metal` を使います。
- `.dmg` 作成時は `scripts/stage-dylibs.sh` が `beforeBundleCommand` から呼ばれ、llama.cpp の dylib をステージングします。

### Linux

配布対象外です（開発用のビルドはできる場合がありますが、サポート外です）。Linux 固有の PR は方針を Issue で相談してください。

## テスト

ネットワーク不要のコアテスト（プルリクエストで CI が実行するのはこれです）。

```bash
cargo test -p overlaysubs-core
```

`overlaysubs-core` はティア選択、モデル定義、PFML3 プロンプト、発話区切り、セッション、SRT 出力を検証します。

実モデルを使うエンドツーエンドのテストは、既定では `ignore` されています。16kHz モノラルの WAV を用意すると実行できます（約 0.7GB のモデルをダウンロードします）。

```bash
OVERLAYSUBS_WAV=path/to/16k-mono.wav cargo test -p overlaysubs -- --ignored
```

変更した範囲に応じて、これらのテストも実行してください。

```bash
cargo test -p overlaysubs          # src-tauri のユニットテスト
cargo build -p overlaysubs         # コンパイルが通ることを確認
```

## ビルドと起動

開発中は Tauri の dev モードで起動するのが速いです。GPU 機能は必要に応じて付けます。

```bash
# 設定画面 + オーバーレイを起動
npx --yes @tauri-apps/cli@^2 dev --features vulkan   # Windows
npx --yes @tauri-apps/cli@^2 dev --features metal    # macOS
```

リリース用バンドル（`.exe` / `.msi` / `.dmg`）を作る場合:

```bash
cd src-tauri
npx --yes @tauri-apps/cli@^2 build --features vulkan   # Windows
npx --yes @tauri-apps/cli@^2 build --features metal    # macOS
```

`cargo install tauri-cli --version "^2" --locked` をして `cargo tauri build` を使っても構いません。NVIDIA GPU 向けは `--features cuda` です。配布物は GitHub Releases のみで公開します。

## コードの配置

| パス | 内容 |
|---|---|
| `crates/core/` | エンジン非依存のロジック。ティア選択（`tier.rs`）、モデル定義（`registry.rs`）、PFML3 プロンプト（`pfml.rs`）、発話区切り（`endpoint.rs`）、セッション（`session.rs`）、SRT/TXT 出力（`export.rs`） |
| `src-tauri/src/` | Tauri アプリ本体。`main.rs`（設定・コマンド・トレイ・ホットキー）、`capture.rs`（WASAPI loopback / マイク）、`engine.rs`（whisper.cpp / llama.cpp アダプタ）、`download.rs`（再開可能・SHA-256 検証付きダウンロード）、`hw.rs`（ハードウェア検出） |
| `src-tauri/capabilities/` | Tauri の権限設定。新しい API を使うときはここも更新します。 |
| `ui/` | 設定画面とオーバーレイ。ビルド不要の素の HTML/JS/CSS。`ui/i18n.js` に日本語と英語の文言があります。 |
| `models.json` | モデル定義（URL・サイズ・SHA-256・ティア）。 |
| `licenses/` | OverlaySubs 利用規約（日英）。PLaMo コミュニティライセンス（2026年11月9日改定）は本文を同梱せず、公式ページへのリンクのみを掲載します。 |
| `scripts/` | リリース・バンドル用の補助スクリプト（`version.sh` は版の表示、`set-version.sh` は版の更新）。 |
| `docs/` | 利用者向けドキュメント（GitHub Pages のソース。英語 `docs/index.html`、日本語 `docs/ja/index.html`、共通 CSS `docs/style.css`。`docs/en/index.html` は旧リンク用のリダイレクトです）。インストール・使い方・FAQ・ライセンスをここに集約します。英語ページは外部 CDN・フォントなしで完結させ、日本語ページのみ Google Fonts を読み込みます。 |

## モデル定義を変更するとき

`models.json` はアプリの動作とダウンロードの完全性に直結します。

- URL は Hugging Face の**コミットに固定**してください。`/resolve/main/` は使わないでください（上流の再アップロードでハッシュが合わなくなります）。このルールは `crates/core` のテストで検証されています。
- `size` と `sha256` は実ファイルと一致させてください。ダウンロードはハッシュ検証に失敗すると一時ファイルを削除します。
- 各ティアにデフォルトの ASR と翻訳モデルが必要です（VAD は全ティア共通）。`Registry::parse` が起動時に検証します。
- サイズを変更した場合は、docs/index.html と docs/ja/index.html（利用者向けのダウンロード量の表）と README のティア表も更新してください。

## UI と言語

- `ui/i18n.js` に日本語（`ja`）と英語（`en`）があります。UI の文言を追加・変更したら**両方**を更新してください。
- 新しい設定項目を追加したら、`ui/app.js` の該当ページと、必要なら `src-tauri/src/main.rs` の `Settings` も更新します。
- 利用規約と PLaMo コミュニティライセンスへのリンクは、ライセンス上の意味があるので勝手に削らないでください。オーバーレイにはクレジットを表示しません。PFN へのクレジットはアプリの「情報」画面、README、公式サイトの「Built with PLaMo」で行うので、こちらは残してください。

## コミットメッセージ

コミットメッセージは、1 行目も本文も英語で書いてください。1 行目で「何をしたか」を簡潔に書き、必要なら本文で理由を説明します。

```
Add Silero VAD-based utterance segmentation
Make model downloads resumable

Fix Windows bundle paths exceeding MAX_PATH
```

大きな変更では、本文に対象 Issue への参照（`#123`）を入れてください。

PR は squash マージされ、そのタイトルがリリースノートの「変更内容」にそのまま使われます。`CHANGELOG.md` への追記は不要です（変更内容はマージされた PR のタイトルから自動生成されます）。

既存の履歴は、このプロジェクトの当初の作業言語である日本語です。書き換えは行わず、これ以降は英語を既定とします。

## プルリクエスト

1. リポジトリを fork し、`main` から作業ブランチを作ります（例: `feat/silero-vad`, `fix/windows-paths`）。
2. 変更をコミットします。
3. `main` に向けて Pull Request を英語のタイトルで作成します（タイトルはリリースノートの変更内容になります）。テンプレートのチェックリストに沿って記入してください。
4. **CI（`cargo test -p overlaysubs-core`）が通ることが必須です。**
5. レビューでの指摘に対応します。

`src-tauri/` やネイティブ依存（whisper.cpp / llama.cpp）、バンドル設定に触る変更では、
レビュー時にメンテナが **`ci:build` ラベル** を付けて Windows / macOS のリリースビルド
（`.github/workflows/release.yml` の `build` ジョブ）を回すことがあります。fork からの PR でも
動き、インストーラがその run の artifact として残るので、マージ前に実機でインストールして
確認できます（`verify` / `publish` はスキップされ、Release は作られません）。ビルドを回して
ほしい場合は、PR の本文にその旨を書いてください。

自分の fork で先に確かめたい場合は、fork 側で同じドライランを実行できます（成果物は fork に
残り、上流リポジトリからは見えません）。

```bash
gh workflow run release.yml --repo <自分のユーザー名>/overlaysubs --ref <作業ブランチ名>
```

PR を出す前に、次を確認してください。

- `cargo test -p overlaysubs-core` が通る。
- 秘密情報（API キー、トークン、個人情報、ローカルのパス）を含めていない。
- UI の文言を変更した場合、`ui/i18n.js` の英語と日本語の両方を更新した。
- 動作やインストール手順が変わる場合、docs/index.html / docs/ja/index.html（利用者向け）と README.md（英語）/ README.ja.md（日本語）（開発者向け）を更新した。
- ライセンスや利用規約（`licenses/`、PLaMo コミュニティライセンスへのリンク）に関わる変更は、その理由を PR に書いた。
- `CHANGELOG.md` への追記は不要（リリースノートは PR タイトルから自動生成されます）。

### フォーマット

- Rust は `cargo fmt` を推奨します（**まだ CI で強制はしていません**）。`cargo clippy` の警告も歓迎します。
- インデントは Rust 4 スペース、JS / HTML / CSS 2 スペース、改行は LF、文字コードは UTF-8 です（[.editorconfig](.editorconfig)）。
- Markdown は末尾の空白を残しても構いません（[.editorconfig](.editorconfig) で `trim_trailing_whitespace = false`）。

### 行動規範

すべての参加者は [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) に従ってください。

### セキュリティ

脆弱性は公開 Issue に書かず、[SECURITY.md](SECURITY.md) の手順で非公開で報告してください。
