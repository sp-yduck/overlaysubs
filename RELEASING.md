# リリース手順

OverlaySubs の配布経路は **GitHub Releases のみ** です。`v*` タグを push すると GitHub Actions
（[`.github/workflows/release.yml`](.github/workflows/release.yml)）が Windows / macOS の
インストーラをビルドし、固定名のアセットとして Release に添付します。

このドキュメントはリポジトリ管理者向けです。利用者向けのドキュメントは公式サイト（[docs/index.html](docs/index.html) / <https://sp-yduck.github.io/overlaysubs/>）にあります。README は開発者向けです。

---

## 1. 仕組み

`v0.1.0` のようなタグを push すると、次の 3 ジョブが順に動きます。

| ジョブ | 条件 | 内容 |
|---|---|---|
| `verify` | タグ push のみ | タグ名 `vX.Y.Z` が root `Cargo.toml` の `[workspace.package] version` と一致するか検証。両クレートが `version.workspace = true` で継承していること、`src-tauri/tauri.conf.json` に `version` が無いことも確認。`productName` が `OverlaySubs` であることも確認 |
| `build` | タグ push、手動実行、ラベル `ci:build` 付き PR | `windows-latest`（Vulkan）と `macos-15`（Metal、ad-hoc 署名）でビルドし、**固定名にリネームしてから** artifact にアップロード |
| `publish` | タグ push のみ | `SHA256SUMS.txt` を生成し、Release を最新として公開。本文はダウンロード表・警告の回避手順・`## Changes`（前回リリース以降の PR タイトルから自動生成） |

Release に添付される固定名アセット（README やランディングページが参照する契約）:

- `overlaysubs-windows-x64-setup.exe` — NSIS インストーラ（Windows の既定。管理者権限不要）
- `overlaysubs-windows-x64.msi` — MSI（管理者配布向け。per-machine インストール）
- `overlaysubs-macos-arm64.dmg` — macOS Apple Silicon 用
- `SHA256SUMS.txt` — 上記 3 ファイルの SHA-256

固定名にしているのは、`https://github.com/sp-yduck/overlaysubs/releases/latest/download/<名前>`
という**バージョンを含まない URL** を README・ランディングページ・Release 本文で使い続けるためです。
名前を変えるときは README.md / README.ja.md / docs/index.html / docs/en/index.html / Release 本文を同時に更新してください。

`workflow_dispatch` は **ビルドのみのドライラン** です。`verify` と `publish` はスキップされ、
Release は作成されません。

**PR でも同じドライランができます。** PR に `ci:build` ラベルを付けると `build` の 2 ジョブだけが
走り、インストーラがその run の artifact として残ります。**fork からの PR でも動く**ので、
マージ前に実機でインストールして試せます。手順と注意点は §6「PR のビルド検証」を参照してください。

---

## 2. はじめて公開するとき（初回のみ）

現在このリポジトリは **private** です。private のままでは Release のファイルを未ログインの人が
ダウンロードできないため、公開が必要です。`gh` は管理者権限（`repo`, `workflow`）でログイン済みで
ある前提です。

> **重要（Actions の課金）**: private リポジトリの Actions は分数を消費し、お支払いが通っていない
> または使用上限が 0 の場合、ジョブが起動せず数秒で失敗します（アノテーション:
> `The job was not started because recent account payments have failed or your spending limit
> needs to be increased`）。**public リポジトリの Actions は無料・無制限**なので、初回は
> 「公開 → 設定 → ドライラン → タグ」の順で進めるのが確実です。

推奨順序:

1. 2.1 公開前チェック
2. 2.4 public に切り替える
3. 2.5〜2.7 リポジトリ設定（説明・トピック・Pages・脆弱性報告・ラベル）
4. 2.2 ビルドのドライラン（public なら無料）
5. 2.8 タグ `v0.1.0` でリリース
6. 2.9 公開後の確認（未ログインでのダウンロード・Pages）

### 2.1 公開前チェック

```bash
# main が最新で CI が緑であること
gh run list --workflow=ci.yml --limit=3

# 秘密情報や巨大ファイルが履歴に無いこと（目視確認）
git log --all --diff-filter=A --name-only --pretty=format: | sort -u | grep -Ei '\.env|secret|credential|\.pem|\.p12|password' || echo "clean"
git rev-list --objects --all | git cat-file --batch-check='%(objecttype) %(objectsize) %(rest)' \
  | awk '$1=="blob" && $2>1000000 {print $2, $3}' | sort -rn | head
```

### 2.2 ビルドのドライラン（public 化後が確実）

タグを打つ前に、実際にインストーラがビルドできるかを確認します。Release は作成されません。
private のままだと Actions の課金設定が必要になるため、**2.4 で public にしてから**実行するのが
確実です（public の Actions は無料）。

```bash
gh workflow run release.yml --ref main
gh run watch "$(gh run list --workflow=release.yml --limit=1 --json databaseId --jq '.[0].databaseId')"

# 生成物のファイル名とサイズを確認（固定名になっているか）
RUN_ID=$(gh run list --workflow=release.yml --limit=1 --json databaseId --jq '.[0].databaseId')
# gh run download は artifact 名のディレクトリに展開されます
gh run download "$RUN_ID" -n overlaysubs-windows-dist
gh run download "$RUN_ID" -n overlaysubs-macos-dist
ls -lh overlaysubs-windows-dist overlaysubs-macos-dist
```

`verify` と `publish` が skipped、`build` の 2 ジョブが success で、上の 3 ファイルが
そろっていれば OK です（この時点の artifact は 7 日で消えます）。

### 2.3 リポジトリ名を overlaysubs に変更する（済）

このリポジトリはすでに **overlaysubs** に改名済みです（旧名 `floatsubs` の URL は GitHub が
リダイレクトします）。手元の clone の remote が旧名のままなら、次で更新してください。

```bash
git remote set-url origin https://github.com/sp-yduck/overlaysubs.git
git remote -v
```

別名で作り直した場合のみ `gh repo rename overlaysubs` を（private のうちに）実行します。

### 2.4 public に切り替える（順番に注意）

ダウンロードリンク・バッジ・Pages は **公開直後から 404 になる期間** があります。避けたい場合は
この節（2.4）より先に「2.8 初回リリース」を private のまま実行し、Release と 4 アセットができてから
public に切り替えてください（その場合、private リポジトリの Actions 分数を消費します。macOS は
10 倍換算です）。public を先にする場合は、続けてすぐタグを打って Release を作ってください。

```bash
gh repo edit sp-yduck/overlaysubs \
  --visibility public \
  --accept-visibility-change-consequences
```

> **注意**: public にすると **これまでのコミット履歴がすべて公開** されます。private に戻すことは
> できますが、一度公開された内容は取り消せません。2.1 のチェックを先に済ませてください。
>
> public にすると **Actions も無料・無制限** になります（private での課金設定は不要）。

### 2.5 リポジトリの説明・トピック・Pages を設定する

```bash
gh repo edit sp-yduck/overlaysubs \
  --description "Offline desktop app that transcribes and translates PC audio in real time and shows it as a subtitle overlay (Tauri / whisper.cpp / PLaMo 3 Translate)" \
  --homepage "https://sp-yduck.github.io/overlaysubs/" \
  --add-topic tauri --add-topic rust --add-topic whisper-cpp --add-topic llama-cpp \
  --add-topic plamo --add-topic speech-recognition --add-topic realtime-translation \
  --add-topic subtitles --add-topic overlay --add-topic desktop-app \
  --add-topic windows --add-topic macos --add-topic japanese

# GitHub Pages（main ブランチの /docs をそのまま配信）
gh api -X POST repos/sp-yduck/overlaysubs/pages \
  --input - <<'JSON'
{"build_type":"legacy","source":{"branch":"main","path":"/docs"}}
JSON

# 反映確認（数十秒〜数分かかることがあります）
curl -sIL -o /dev/null -w '%{http_code}\n' https://sp-yduck.github.io/overlaysubs/
```

GitHub の画面から設定する場合: **Settings → Pages → Build and deployment → Source =
"Deploy from a branch"、Branch = `main`、フォルダ = `/docs`**。`docs/.nojekyll` を置いてあるので
Jekyll の処理は入りません。

### 2.6 脆弱性の非公開報告を有効にする

[SECURITY.md](SECURITY.md) は GitHub の Private vulnerability reporting を前提にしています。

```bash
gh api -X PUT repos/sp-yduck/overlaysubs/private-vulnerability-reporting
```

画面からは **Settings → Code security → Private vulnerability reporting** を Enable。

### 2.7 依存更新 PR 用のラベルを作る（任意）

`.github/dependabot.yml` は `dependencies` / `rust` / `github-actions` ラベルを付けます。
無い場合ラベルは付きませんが、Dependabot 自体は動作します。

```bash
gh label create dependencies    -R sp-yduck/overlaysubs --color 0366d6 --description "Dependency updates"
gh label create rust            -R sp-yduck/overlaysubs --color dea584 --description "Rust / Cargo"
gh label create github-actions  -R sp-yduck/overlaysubs --color 2088ff --description "GitHub Actions"
```

### 2.8 初回リリース（v0.1.0）

```bash
git switch main && git pull
git tag -a v0.1.0 -m "OverlaySubs v0.1.0"
git push origin v0.1.0

# 進行確認（Windows / macOS のビルドで 30〜60 分ほどかかります）
gh run watch "$(gh run list --workflow=release.yml --limit=1 --json databaseId --jq '.[0].databaseId')"
```

### 2.9 公開後の確認

```bash
gh release view v0.1.0

# 「GitHub にログインしていない人」でもダウンロードできることを確認する
# （curl は認証ヘッダを付けないので、これがそのまま未ログイン検証になります）
for f in overlaysubs-windows-x64-setup.exe overlaysubs-windows-x64.msi \
         overlaysubs-macos-arm64.dmg SHA256SUMS.txt; do
  printf '%s: ' "$f"
  curl -sIL -o /dev/null -w '%{http_code}\n' \
    "https://github.com/sp-yduck/overlaysubs/releases/latest/download/$f"
done

# ランディングページ
curl -sIL -o /dev/null -w '%{http_code}\n' https://sp-yduck.github.io/overlaysubs/
```

4 アセットすべて **200**、Pages も **200** なら公開完了です。README のバッジ
（CI / latest release）もこの時点から表示されます。

---

## 3. 2 回目以降のリリース

バージョンは root `Cargo.toml` の `[workspace.package] version` の 1 箇所だけです。両クレートは
`version.workspace = true` で継承し、`src-tauri/tauri.conf.json` には `version` を置かないため、
Tauri は Cargo の版を使います。

1. **バージョンを 1 箇所で上げる**（スクリプトが `Cargo.lock` も更新します）

   ```bash
   scripts/set-version.sh X.Y.Z
   git diff --stat
   ```

2. テストとビルド確認

   ```bash
   cargo test -p overlaysubs-core
   gh workflow run release.yml --ref <ブランチ名>   # ドライラン（Release は作られない）
   ```

3. PR を作って `main` にマージしたあと、タグを push

   ```bash
   git switch main && git pull
   git tag -a vX.Y.Z -m "OverlaySubs vX.Y.Z"
   git push origin vX.Y.Z
   ```

> `CHANGELOG.md` はありません。Release 本文の `## Changes` は、前回のリリース以降にマージされた
> PR のタイトルから自動生成されます（`chore(release)` は除く）。利用者に伝わるよう、PR タイトルは
> 英語で付けてください。

---

## 4. トラブルシューティング

### ビルドが失敗した

Actions のログを確認します。よくある原因:

- Windows: Vulkan SDK のダウンロード失敗 → ワークフローを再実行
- Windows: `llama.dll` のステージング失敗 → `src-tauri/libs/` と `C:\t\release\build\llama-cpp-sys-2-*\out\bin\`
- macOS: `codesign` 検証失敗 → ad-hoc 署名の設定（`--config '{"bundle":{"macOS":{"signingIdentity":"-","hardenedRuntime":false}}}'`）が消えていないか。ad-hoc 署名では hardened runtime を無効にしないと、同梱した llama/ggml の dylib が library validation で拒否されて起動時にクラッシュします
- ビルド時間切れ（240 分）→ キャッシュが効いていれば通常は数十分

失敗したジョブだけを再実行できます。

```bash
gh run rerun <run-id> --failed
```

### 配布した版が一部の PC で「不正命令」で落ちる（0xc000001d）

リリースビルドが GitHub ランナーの CPU 向けにコンパイルされてしまうと、その命令（AVX-512 など）を
持たない CPU では `STATUS_ILLEGAL_INSTRUCTION (0xc000001d)` で即終了します。イベントビューアの
「アプリケーション エラー」で **例外コード `0xc000001d`・障害モジュールがアプリ本体**ならこれです。
whisper.cpp / ggml は `GGML_NATIVE` が既定で ON のため、明示的に OFF にしないとビルドマシンの CPU に
最適化されます（llama.cpp は既定で OFF、whisper.cpp は `release.yml` の build ジョブで
`GGML_NATIVE=OFF` を渡しています）。

`GGML_*` / `CMAKE_*` の設定を変えたときは、rust-cache の `key` の接尾辞（現在は
`-native-off-no-openssl`）も更新してください。キャッシュが復元されると C/C++ のオブジェクトが再利用され、
設定変更が反映されません。

### macOS 版が起動直後に落ちる（`Library not loaded: /opt/homebrew/...`）

配布した macOS 版が、Homebrew を入れていない Mac で「問題が起きたため OverlaySubs を開けません」と
表示されて終了する場合、同梱した llama の dylib がビルドマシン（GitHub ランナー）の
Homebrew の OpenSSL を絶対パスで参照しています。llama.cpp の `LLAMA_OPENSSL` は既定で ON で、
`vendor/cpp-httplib` が `find_package(OpenSSL)` で見つけた OpenSSL を `libllama-common` にリンクするため
（v0.2.4 で発生: issue #35）、Homebrew の無い Mac では `libssl.4.dylib` を読み込めずに dyld が
`Library missing` で終了します。

`release.yml` の build ジョブは `CMAKE_DISABLE_FIND_PACKAGE_OpenSSL=ON` を渡して検出そのものを止め、
`Verify macOS bundle is signed` ステップで **バンドル内の全 Mach-O** の依存を検査して、macOS の
システムライブラリ以外の絶対パスや、未同梱の `@rpath` があれば失敗するようにしています。
この設定を変えたときも rust-cache の `key` の接尾辞を更新してください。

システム音声キャプチャ（ScreenCaptureKit）のバインディングは、ビルド時に小さな Swift ブリッジを
静的リンクするため、実行時には macOS 同梱の Swift ランタイム（`/usr/lib/swift`）を `@rpath` で参照します。
rpath は `src-tauri/build.rs` がアプリ本体に付与し（依存クレートの build script が出力する
`rustc-link-arg` はアプリ本体のリンクに伝播しないため）、検査は Mach-O が `LC_RPATH /usr/lib/swift` を
持つ場合に限り `@rpath/libswift*` を許可します。macOS 13 以降は OS が Swift ランタイムを提供するため
同梱は不要です。起動時に `Library not loaded: @rpath/libswift_Concurrency.dylib` で落ちる場合は、
`src-tauri/build.rs` の macOS 向け `rustc-link-arg` が消えていないか確認してください。

### macOS のビルドが `cannot find 'MTLTensorDataType' in scope` で失敗する

screencapturekit が依存する apple-metal の Swift ブリッジは、macOS 26 SDK で追加された Metal API
（`MTLTensor*`、`MTLSamplerReductionMode` など）を参照するため、既定の Xcode 16.4（macOS 15.5 SDK）
ではビルドできません。`release.yml` の macOS ビルドは、`macos-15` ランナーにプリインストールされた
Xcode 26 を `xcode-select` で選択してからビルドします。手元でビルドする場合も
`sudo xcode-select -s /Applications/Xcode_26.x.app` としてください（配布物の macOS 13 対応は、
tauri-cli が `MACOSX_DEPLOYMENT_TARGET=13.0` を設定するため変わりません）。

### 同じバージョンで作り直したい

Release に 4 アセットがそろっていると、`verify` が「公開済み」と判断して **ビルドをスキップ** します
（二重公開・タグの取り違えを防ぐため）。作り直すときは Release とタグを消してから打ち直します。

```bash
gh release delete vX.Y.Z --yes
git push origin :refs/tags/vX.Y.Z
git tag -d vX.Y.Z
git tag -a vX.Y.Z -m "OverlaySubs vX.Y.Z"
git push origin vX.Y.Z
```

### CI / リリースのジョブが数秒で失敗する

ジョブが `steps=0` で失敗し、アノテーションに次が出る場合、Actions の課金設定が原因です。

> The job was not started because recent account payments have failed or your spending limit
> needs to be increased.

対処: **リポジトリを public にする**（public の Actions は無料）か、GitHub の
**Settings → Billing & plans** でお支払い方法・使用上限を設定してください。private のまま
リリースする場合も、この設定がないとタグを打ってもビルドが起動しません。

### タグを打ったのに何も起きない

- タグが `v0.1.0` 形式か（`0.1.0` や `release-0.1.0` は対象外）
- `release.yml` が `main` に存在するか（`workflow_dispatch` は default branch にファイルが
  無いと手動実行できません）
- コミットが `main` にマージ済みか

### アセット名や URL を変えたい

固定名は README.md / README.ja.md / docs/index.html / docs/en/index.html / Release 本文 / このドキュメントの契約です。
`release.yml` のステージング名・サイズ下限・`verify` の `release_exists` 期待値を同時に更新し、
利用者向けのリンクもすべて揃えてからタグを打ってください。

---

## 5. コード署名を追加する場合

現在の配布ビルドは **Windows のコード署名なし / macOS は ad-hoc 署名のみ（公証なし）** のため、
SmartScreen と Gatekeeper の警告が出ます（[公式サイトの「警告が表示されたとき」](https://sp-yduck.github.io/overlaysubs/#warning)）。
警告を無くしたい場合は次の secret を設定すると、ワークフローを変更せずに署名へ切り替わります。

### macOS（Apple Developer Program が必要）

| Secret | 内容 |
|---|---|
| `APPLE_CERTIFICATE` | Developer ID Application 証明書（.p12）を base64 にしたもの |
| `APPLE_CERTIFICATE_PASSWORD` | .p12 のパスワード |
| `APPLE_SIGNING_IDENTITY` | 証明書の identity 名（これが設定されると ad-hoc 署名は使われません） |
| `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID` | 公証（notarization）を行う場合 |

### Windows

コード署名証明書（OV / EV）を用意し、`src-tauri/tauri.conf.json` の
`bundle.windows.certificateThumbprint` などを設定します。

署名を入れるまでは、公式サイトの「警告が表示されたとき」（<https://sp-yduck.github.io/overlaysubs/#warning>）を案内してください。

---

## 6. PR のビルド検証（フォークからの PR を含む）

PR に **`ci:build` ラベル** を付けると、`release.yml` の `build` ジョブだけが走ります
（`verify` / `publish` はスキップ）。マージ前に Windows / macOS のインストーラを実際に作って
インストールできるので、Tauri のバンドルやネイティブ依存（whisper.cpp / llama.cpp）に触る変更の
確認に使えます。**fork からの PR でも同じように動きます。**

### 6.1 初回だけ: ラベルを作る

```bash
gh label create "ci:build" -R sp-yduck/overlaysubs \
  --color 1d76db --description "Run the Windows / macOS release build for this PR"
```

### 6.2 実行する

```bash
# PR にラベルを付ける（メンテナの操作。fork PR でも可）
gh pr edit <PR番号> --add-label "ci:build"
```

- ラベルを付けた時点で 1 回目のビルドが始まります。以後その PR に push されるたびに
  自動で再ビルドします（`synchronize`）
- 同じ PR で再実行したいときは、ラベルを外して付け直してください（`labeled` が再度発生します）
- **初回コントリビュータの PR は承認が必要**です。run 画面の「Approve and run workflows」を
  押すか、Settings → Actions → General の「Fork pull request workflows from outside
  collaborators」の設定に従ってください
- 同じ PR に新しい commit を push すると、**前の run は自動でキャンセル**されます
  （タグ push の run はキャンセルされません）
- このワークフローの変更が `main` にマージされる前の PR では、ラベルを付けてもビルドは
  走りません（`pull_request` の run は PR のマージコミットにあるワークフロー定義で動くため）

### 6.3 成果物を取得して試す

```bash
# PR の run を探す（fork のブランチでも --event で絞れば確実）
RUN_ID=$(gh run list --workflow=release.yml --event=pull_request \
  --limit=1 --json databaseId --jq '.[0].databaseId')
gh run view "$RUN_ID"

gh run download "$RUN_ID" -n overlaysubs-windows-dist
gh run download "$RUN_ID" -n overlaysubs-macos-dist
ls -lh overlaysubs-windows-dist overlaysubs-macos-dist
```

固定名の 3 アセット（`overlaysubs-windows-x64-setup.exe` / `overlaysubs-windows-x64.msi` /
`overlaysubs-macos-arm64.dmg`）がそろっていれば OK です。PR の artifact は **3 日**で消えます
（ドライランは 7 日）。

### 6.4 制約と注意

- **secrets は渡りません**。`GITHUB_TOKEN` も read-only に固定されるため、`publish` は
  仮にワークフローを書き換えても実行できません。macOS は ad-hoc 署名（公証なし）で
  ビルドされるので、Gatekeeper の警告の出方は配布ビルドと同じですが、Developer ID 署名や
  公証そのものの検証はできません。署名付きで確認したい変更は、メンテナが一時ブランチに
  取り込んで `gh workflow run release.yml --ref <ブランチ>` でドライランしてください
- **`publish` は絶対に走りません**。タグ push のときだけです。PR の artifact はテスト専用で、
  Release には添付されません
- PR は `release.yml` 自体も書き換えられるため、**PR のビルドが緑でもリリースの根拠には
  しない**でください。正となるのは `main` のタグ push で作ったビルドです
- PR ビルドはキャッシュを保存しません（リリース用キャッシュを追い出さないため）。そのぶん
  1 回目は時間がかかります
- 2 OS × 最大 4 時間の timeout です。public リポジトリなら Actions の分数は無料ですが、
  ラベルで必要なときだけ回す運用にしてください
