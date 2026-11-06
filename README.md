# floatsubs

[![CI](https://github.com/sp-yduck/floatsubs/actions/workflows/ci.yml/badge.svg)](https://github.com/sp-yduck/floatsubs/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/sp-yduck/floatsubs?label=release&sort=semver)](https://github.com/sp-yduck/floatsubs/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Windows x64](https://img.shields.io/badge/platform-Windows%20x64-0078d4.svg)](#動作環境)
[![macOS Apple Silicon](https://img.shields.io/badge/platform-macOS%20Apple%20Silicon-lightgrey.svg)](#動作環境)

PC 上で再生されるあらゆる音声をリアルタイムで文字起こし・翻訳し、字幕としてオーバーレイ表示するデスクトップアプリです。
**インストールして起動するだけ。API キー不要・完全無料。**

**Built with PLaMo.** 翻訳には Preferred Networks の [PLaMo 3 Translate](https://huggingface.co/pfnet/plamo-3-translate-0.8b-GGUF) を使います。

## 目次

- [ダウンロード](#ダウンロード)
- [使い方](#使い方)
- [仕組み](#仕組み)
- [機能](#機能)
- [ティア](#ティア)
- [トラブルシューティング（FAQ）](#トラブルシューティングfaq)
- [現状の制限](#現状の制限)
- [ビルド](#ビルド)
- [開発に参加する](#開発に参加する)
- [ライセンス](#ライセンス)

## ダウンロード

**GitHub アカウントは不要です。** 下のリンクをクリックしてファイルをダウンロードし、実行してください。
（ブラウザで見たい方は [公式ページ](https://sp-yduck.github.io/floatsubs/) からもダウンロードできます。）

| お使いの PC | ダウンロード | 備考 |
|---|---|---|
| **Windows 10 / 11（x64）** | [**floatsubs-windows-x64-setup.exe**](https://github.com/sp-yduck/floatsubs/releases/latest/download/floatsubs-windows-x64-setup.exe) | 通常はこちら（インストーラ） |
| Windows（MSI が必要な場合） | [floatsubs-windows-x64.msi](https://github.com/sp-yduck/floatsubs/releases/latest/download/floatsubs-windows-x64.msi) | 社内配布などで MSI が必要な場合の代替 |
| **macOS 13 以降（Apple Silicon）** | [**floatsubs-macos-arm64.dmg**](https://github.com/sp-yduck/floatsubs/releases/latest/download/floatsubs-macos-arm64.dmg) | M1 / M2 / M3 / M4。Intel Mac 版はありません |

- リリースページ: <https://github.com/sp-yduck/floatsubs/releases/latest>
- 過去のバージョンを含む一覧: <https://github.com/sp-yduck/floatsubs/releases>
- チェックサム: [SHA256SUMS.txt](https://github.com/sp-yduck/floatsubs/releases/latest/download/SHA256SUMS.txt)

> MSI は管理者権限（全ユーザー向け）でインストールされます。通常は `.exe`（現在のユーザー向け）を推奨します。

### インストール（3 ステップ）

1. **ダウンロード** — 上の表から、お使いの PC に合ったファイルをクリックします。
2. **実行** — ダウンロードしたファイルを開きます。Windows は `.exe` をそのまま実行、macOS は `.dmg` を開いて「floatsubs」を「アプリケーション」フォルダにドラッグします。
3. **初回セットアップ** — 起動したら、利用規約に同意 →「PC の性能を調べる」→「モデルをダウンロード」の順に進めます（数分〜十数分かかります）。

初回だけモデルのダウンロードが必要です。2 回目以降はすぐに翻訳を始められます。

### 警告が表示されたとき（未署名・未公証のビルド）

配布しているビルドは**未署名・未公証**です（Windows はコード署名なし、macOS は Apple の Developer ID 署名なし・公証なしで **ad-hoc 署名のみ**）。そのため初回起動時に OS が警告（Windows の **SmartScreen / スマートスクリーン**、macOS の **Gatekeeper / ゲートキーパー**）を表示しますが、**アプリ自体に問題があるという意味ではありません**。次の手順で起動できます。

**Windows: 「Windows によって PC が保護されました」と表示された**

1. 青い画面の **「詳細情報」** をクリックします。
2. 表示された **「実行」** ボタンをクリックします。

**macOS: 「開発元を確認できません」/「開いていません」と表示された**

1. Finder で floatsubs を **右クリック（または Control キーを押しながらクリック）** → **「開く」** → 確認ダイアログで **「開く」**。
2. それでも開かない場合（macOS 15 以降など）は、**システム設定 → プライバシーとセキュリティ** を開き、下部に出る「floatsubs は…ブロックされました」の横の **「このまま開く」** をクリックします。

> [!WARNING]
> Gatekeeper（ゲートキーパー）や SmartScreen（スマートスクリーン）を**システム全体で無効にする**のはやめてください。上記のように、このアプリだけを個別に許可する手順を使ってください。macOS 15 以降では右クリック →「開く」の回避操作が廃止されたため、システム設定からの操作が必要です。

### 動作環境

| | Windows | macOS |
|---|---|---|
| OS | Windows 10 / 11（64bit） | macOS 13 Ventura 以降 |
| CPU | x64（Intel / AMD） | Apple Silicon（M1 以降） |
| GPU | **Vulkan 対応 GPU + 最新の GPU ドライバ** | 内蔵 GPU（Metal） |
| メモリ | 8GB 以上 | 8GB 以上 |
| 空き容量 | 下の「初回起動時のモデルダウンロード」を参照 | 同左 |

- Windows 版は **Vulkan 対応の GPU と最新ドライバ**が必要です。ドライバが古いと GPU が使われず、実用的な速度が出ません。NVIDIA / AMD / Intel の各公式サイトから最新ドライバを入れてください。
- Intel Mac 版はありません。32bit Windows、Windows on ARM 版もありません。
- 初回セットアップで「PC の性能を調べる」を実行すると、GPU の VRAM（Apple Silicon はメモリ）から High / Mid / Low / CPU のティアが決まり、モデルが自動選択されます。目安は[ティア](#ティア)の表を参照してください。
- 配布は **GitHub Releases のみ**です（自動アップデート機能はありません）。

### 初回起動時のモデルダウンロード（数 GB）

初回セットアップで、PC の性能に合わせたモデルを Hugging Face から自動でダウンロードします。
**Hugging Face のアカウントもトークンも不要です。** ダウンロードは中断しても続きから再開でき、SHA-256 で検証されます。

| ティア | 自動で選ばれる構成 | ダウンロード量 | 空き容量の目安 |
|---|---|---|---|
| High | Whisper large-v3-turbo + PLaMo 3 Translate 8B (Q4_K_M) | 約 6.9GB | 10GB 以上 |
| Mid | Whisper large-v3-turbo (Q5_0) + PLaMo 3 Translate 2B (Q4_K_M) | 約 2.4GB | 5GB 以上 |
| Low | Whisper small + PLaMo 3 Translate 2B (Q4_K_M) | 約 2.3GB | 5GB 以上 |
| CPU | Whisper base + PLaMo 3 Translate 0.8B (Q4_K_M) | 約 0.7GB | 2GB 以上 |

数値は `models.json` の宣言サイズの合計（Silero VAD の約 0.9MB を含む）です。初回セットアップで「PC の性能を調べる」を実行すると、ハードウェアからティアが決まりモデルが自動選択されます。設定画面では使用するモデル（Whisper / PLaMo）を個別に変更できます。

### SHA-256 の確認（任意）

ダウンロードしたファイルが壊れていないか確認できます。表示された値を [SHA256SUMS.txt](https://github.com/sp-yduck/floatsubs/releases/latest/download/SHA256SUMS.txt) の値と比べてください。

**Windows（PowerShell）**

```powershell
Get-FileHash .\floatsubs-windows-x64-setup.exe -Algorithm SHA256
```

**macOS（ターミナル）**

```bash
shasum -a 256 floatsubs-macos-arm64.dmg
```

### アンインストール

**Windows**: 設定 → アプリ → インストールされているアプリ →「floatsubs」→ アンインストール

**macOS**: アプリケーションフォルダの「floatsubs」をゴミ箱に入れる

アンインストールしても、ダウンロード済みモデルと設定は次のフォルダに残ります（再インストール時に再ダウンロードは不要です）。完全に削除したい場合は手動で削除してください。

| OS | 場所 |
|---|---|
| Windows | `%APPDATA%\io.github.floatsubs` |
| macOS | `~/Library/Application Support/io.github.floatsubs` |

このフォルダには、モデル（`models`）、設定（`settings.json`）、利用規約への同意（`terms-accepted`）が保存されます。アプリの識別子は `io.github.floatsubs` です。

## 使い方

1. 初回起動時に、利用規約（PLaMo コミュニティライセンスを別紙として含む）に同意し、モデルを準備します。
2. 音声入力を選びます。Windows は「システム音声」で PC の再生音をそのまま取り込めます。macOS ではループバック（仮想オーディオ）デバイスを「マイク」として選んでください（[現状の制限](#現状の制限)）。
3. **`Ctrl+Shift+T`（macOS は `Cmd+Shift+T`）** で翻訳を開始・停止します。ホットキーは設定で変更できます。
4. 字幕は最前面・半透明のオーバーレイに表示されます。クリック透過（マウス操作を下の画面に通す）は「字幕の表示」でオンにできます。位置や見た目もここで調整できます。
5. 終了はタスクトレイ / メニューバーのアイコン →「Quit」です。設定画面を閉じるとアプリは終了します。

音声・文字起こし・翻訳データは外部に送信しません。通信はモデルのダウンロードだけです。テレメトリはありません。モデルをダウンロードした後はオフラインでも動作します。

## 仕組み

```
WASAPI loopback / マイク ─▶ 16kHz mono ─▶ エネルギー区切り ─▶ whisper.cpp (+ Silero VAD) ─▶ PLaMo 3 Translate (llama.cpp) ─▶ オーバーレイ
```

- `crates/core` はエンジン非依存のロジックです。ティア選択、モデル定義、PFML3 プロンプト、発話区切り、セッション、SRT 出力を持ちます。`cargo test -p floatsubs-core` で検証できます。
- `src-tauri` は Tauri アプリ本体です。音声キャプチャ、whisper.cpp / llama.cpp アダプタ、ダウンロード、トレイ、ホットキーを担当します。
- `ui` は設定画面とオーバーレイです。ビルド不要の素の HTML/JS です。
- 翻訳エンジンと音声認識は `session::Translator` / `session::Asr` トレイトの裏にあり、差し替えられます。
- `models.json` がモデル定義（URL、サイズ、SHA-256、ティア）です。URL は Hugging Face のコミットに固定しています（`main` だと上流の再アップロードでハッシュが合わなくなるため）。設定フォルダに `models.json` を置くと、アプリを更新せずに上書きできます。

## 機能

- 初回起動で floatsubs 利用規約（PLaMo コミュニティライセンスを別紙として含む）に同意します。同意は設定フォルダの `terms-accepted` に保存し、規約の版が変わるまで再表示しません。「PC の性能を調べる」でハードウェアを検出し、ティア（High / Mid / Low / CPU）に応じたモデルを自動選択します。
- モデルは Hugging Face から自動ダウンロードします。中断しても再開でき、SHA-256 で検証します。アカウントやトークンは不要です。
- 任意の速度チェック（ベンチマーク）を実行でき、リアルタイムに間に合わなければ1段下げたモデルに切り替えます。
- 入力言語の自動判定または固定。訳文言語と「入力が訳文と同じ言語のとき」の言語を選べるので、英⇔日・韓⇔日を同時に扱えます。
- 直前3発話を文脈として渡します。用語集と日本語の敬体／常体を指定できます。
- 暫定結果は薄く表示し、確定訳はストリーミングで上書きします。
- 最前面・半透明のオーバーレイ（クリック透過は設定でオンにできます）。原文併記、文字サイズ、フォント、背景透過度、行数、表示時間、「Built with PLaMo」表記の有無を変更できます。
- 履歴をテキスト／SRT で保存できます。
- トレイ常駐、グローバルホットキー（既定 `CmdOrCtrl+Shift+T`。Windows は `Ctrl+Shift+T`、macOS は `Cmd+Shift+T`）で開始・停止。UI は日本語・英語。
- 音声・文字起こし・翻訳データは外部に送信しません。通信はモデルのダウンロードだけです。テレメトリはありません。

## ティア

ティアは初回セットアップで「PC の性能を調べる」を実行したときに決まり、そのティアに応じたモデルが自動で選択されます。

| ティア | 判定 | Whisper | PLaMo 3 Translate |
|---|---|---|---|
| High | VRAM 12GB以上 / Apple Silicon RAM 32GB以上 | large-v3-turbo | 8B Q4_K_M |
| Mid | VRAM 8GB / Apple Silicon RAM 16GB | large-v3-turbo Q5_0 | 2B Q4_K_M |
| Low | VRAM 4〜6GB / Apple Silicon RAM 8GB | small | 2B Q4_K_M |
| CPU | GPU なし、または VRAM 4GB 未満 | base | 0.8B Q4_K_M |

Mid で 8B ではなく 2B を選んでいるのは、8GB の VRAM に Whisper と 8B Q4_K_M（5.3GB）を同居させるとゲームと併用する余裕がないためです。設定画面で 8B Q3_K_M や 31B に手動で変更できます。

## トラブルシューティング（FAQ）

**Q. Windows で「Windows によって PC が保護されました」と出て実行できない**
A. 未署名のビルドのため表示されます。「詳細情報」→「実行」で起動できます（[警告が表示されたとき](#警告が表示されたとき未署名未公証のビルド)）。

**Q. macOS で「開発元を検証できません」と出て開けない**
A. 右クリック →「開く」を試してください。開けない場合は システム設定 → プライバシーとセキュリティ →「このまま開く」を使います（[警告が表示されたとき](#警告が表示されたとき未署名未公証のビルド)）。

**Q. 起動したが翻訳を開始できない**
A. 初回セットアップが完了していません。「モデルの準備」で「PC の性能を調べる」→「モデルをダウンロード」まで進めてください。ダウンロードが終わるまで開始ボタンは使えません。

**Q. ホットキーが効かない**
A. 他のアプリが同じキーを使っている可能性があります。設定画面で `CmdOrCtrl+Shift+T` を別の組み合わせに変更してください。

**Q. GPU が使われていない / 動作が遅い**
A. Windows では Vulkan 対応の最新 GPU ドライバが必要です。ドライバを更新しても遅い場合は、設定画面で使用するモデル（Whisper / PLaMo）を軽いものに変更するか、「速度を測る」を実行して1段下げたモデルに切り替えてください。CPU 向けのモデルでも動作しますが遅くなります。

**Q. macOS でシステム音声を取り込めない**
A. macOS のシステム音声キャプチャ（ScreenCaptureKit）は未実装です。BlackHole などのループバック（仮想オーディオ）デバイスを用意し、入力元を「マイク」にしてそのデバイスを選んでください（[現状の制限](#現状の制限)）。

**Q. モデルのダウンロードに失敗する**
A. 通信が切れても、もう一度ダウンロードすると続きから再開します。ハッシュ検証に失敗した一時ファイルは自動で削除されます。空き容量（[初回起動時のモデルダウンロード](#初回起動時のモデルダウンロード数-gb)）とネットワークを確認して、再度ダウンロードしてください。モデルの保存先は設定画面の「モデルの準備」→「保存先」で確認できます。

**Q. オフラインでも使える？**
A. 使えます。モデルのダウンロードが終わった後は、ネットワークがなくても文字起こし・翻訳が動作します。

**Q. 商用利用できる？**
A. アプリのコードは MIT ライセンスです。翻訳モデル PLaMo は PLaMo コミュニティライセンスに従い、個人・非商用は無償、商用利用には PFN への事前登録が必要です。年間売上が10億円を超える組織の業務利用は別途契約が必要です（[ライセンス](#ライセンス)）。

**Q. 字幕を動画や配信で公開してよい？**
A. 公開・配布する場合は「Built with PLaMo」と明記してください。保存したテキスト／SRT には表記が入らないため、公開時にご自身で明記が必要です。元コンテンツの著作権にも注意してください。

**Q. アンインストールしたのに容量が減らない**
A. モデルと設定が上記のフォルダに残っています。手動で削除してください（[アンインストール](#アンインストール)）。

## 現状の制限

- 配布ビルドは**未署名・未公証**です（Windows のコード署名なし。macOS は Developer ID 署名・公証なしの **ad-hoc 署名のみ**）。そのため SmartScreen / Gatekeeper の警告が表示されます。
- Intel Mac 版はありません（macOS は Apple Silicon のみ、macOS 13 以降）。
- macOS のシステム音声キャプチャ（ScreenCaptureKit）は未実装です。macOS ではループバック入力デバイスを「マイク」として選んでください。
- 特定アプリだけのキャプチャ、システム音声とマイクの同時処理は未実装です。
- 自動アップデートはありません。新しいバージョンは GitHub Releases から手動でダウンロードしてください。
- 配布は GitHub Releases のみです。

## ビルド

```bash
cargo install tauri-cli --version "^2" --locked
cd src-tauri && cargo tauri build --features vulkan   # Windows
cd src-tauri && cargo tauri build --features metal    # macOS
```

Tauri CLI は `npx --yes @tauri-apps/cli@^2 build --features vulkan` でも実行できます（CI はこちらを使用）。NVIDIA 向けには `--features cuda` を使います。whisper.cpp と llama.cpp はどちらも ggml を含むため、llama.cpp は共有ライブラリとしてリンクし、インストーラに同梱します。

開発環境の準備、テスト、PR の流れは [CONTRIBUTING.md](CONTRIBUTING.md) を参照してください。

## 開発に参加する

- [CONTRIBUTING.md](CONTRIBUTING.md) — 開発環境、テスト、PR の流れ
- [SECURITY.md](SECURITY.md) — 脆弱性の報告方法（非公開で受け付けます）
- [CHANGELOG.md](CHANGELOG.md) — 変更履歴
- [RELEASING.md](RELEASING.md) — リリース手順（管理者向け）
- [docs/index.html](docs/index.html) — 配布用ランディングページ（GitHub Pages）
- Issue テンプレート: [バグ報告](.github/ISSUE_TEMPLATE/bug_report.yml) / [機能要望](.github/ISSUE_TEMPLATE/feature_request.yml)

## ライセンス

- **アプリのコードは MIT ライセンスです**（[LICENSE](LICENSE)）。
- アプリの利用条件は [floatsubs 利用規約](licenses/floatsubs-terms.ja.txt)（[English](licenses/floatsubs-terms.en.txt)）です。規約は [PLaMo コミュニティライセンス](https://plamo.preferredai.jp/info/plamo-community-license) を別紙として含み、初回起動時にまとめて同意を求めます。全文を [`licenses/`](licenses/) に同梱し、アプリの「情報」からも参照できます。
  - 個人・非商用の利用は無償です。
  - 商用利用には事前登録が必要です。また、本人または関連会社の年間売上が10億円を超える組織が業務で使う場合は、Preferred Networks との別途契約が必要です。
  - 字幕などの出力を公開・配布する場合は、PLaMo による出力である旨を明示してください。保存したテキスト/SRT には表記を入れないので、公開時はご自身で明記してください。字幕オーバーレイには既定で `Built with PLaMo` を表示します（設定でオフにできます）。
- 配信や動画の字幕を公開する場合は、元コンテンツの著作権にも注意してください。
- Whisper モデル（MIT）と Silero VAD（MIT）は、それぞれのライセンスに従います。
- 同梱している whisper.cpp / llama.cpp / ggml などのサードパーティライセンスは [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) にまとめています。
