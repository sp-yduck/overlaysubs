# リリース手順

floatsubs の配布経路は **GitHub Releases のみ** です。`v*` タグを push すると GitHub Actions
（[`.github/workflows/release.yml`](.github/workflows/release.yml)）が Windows / macOS の
インストーラをビルドし、固定名のアセットとして Release に添付します。

このドキュメントはリポジトリ管理者向けです。利用者向けのドキュメントは公式サイト（[docs/index.html](docs/index.html) / <https://sp-yduck.github.io/floatsubs/>）にあります。README は開発者向けです。

---

## 1. 仕組み

`v0.1.0` のようなタグを push すると、次の 3 ジョブが順に動きます。

| ジョブ | 条件 | 内容 |
|---|---|---|
| `verify` | タグ push のみ | タグ名 `vX.Y.Z` が `src-tauri/tauri.conf.json` / `src-tauri/Cargo.toml` / `crates/core/Cargo.toml` の version と一致するか検証。`productName` が `floatsubs` であることも確認 |
| `build` | タグ push と手動実行 | `windows-latest`（Vulkan）と `macos-15`（Metal、ad-hoc 署名）でビルドし、**固定名にリネームしてから** artifact にアップロード |
| `publish` | タグ push のみ | `SHA256SUMS.txt` を生成し、Release を最新として公開。本文はダウンロード表・警告の回避手順・CHANGELOG の該当セクション |

Release に添付される固定名アセット（README やランディングページが参照する契約）:

- `floatsubs-windows-x64-setup.exe` — NSIS インストーラ（Windows の既定。管理者権限不要）
- `floatsubs-windows-x64.msi` — MSI（管理者配布向け。per-machine インストール）
- `floatsubs-macos-arm64.dmg` — macOS Apple Silicon 用
- `SHA256SUMS.txt` — 上記 3 ファイルの SHA-256

固定名にしているのは、`https://github.com/sp-yduck/floatsubs/releases/latest/download/<名前>`
という**バージョンを含まない URL** を README・ランディングページ・Release 本文で使い続けるためです。
名前を変えるときは README / README.en.md / docs/index.html / Release 本文を同時に更新してください。

`workflow_dispatch` は **ビルドのみのドライラン** です。`verify` と `publish` はスキップされ、
Release は作成されません。

---

## 2. はじめて公開するとき（初回のみ）

現在このリポジトリは private です。private のままでは Release のファイルを未ログインの人が
ダウンロードできないため、公開には次の手順が必要です。`gh` は管理者権限（`repo`, `workflow`）で
ログイン済みである前提です。

### 2.1 公開前チェック

```bash
# main が最新で CI が緑であること
gh run list --workflow=ci.yml --limit=3

# 秘密情報や巨大ファイルが履歴に無いこと（目視確認）
git log --all --diff-filter=A --name-only --pretty=format: | sort -u | grep -Ei '\.env|secret|credential|\.pem|\.p12|password' || echo "clean"
git rev-list --objects --all | git cat-file --batch-check='%(objecttype) %(objectsize) %(rest)' \
  | awk '$1=="blob" && $2>1000000 {print $2, $3}' | sort -rn | head
```

### 2.2 ビルドのドライラン（公開前でも実行可能）

タグを打つ前に、実際にインストーラがビルドできるかを確認します。Release は作成されません。

```bash
gh workflow run release.yml --ref main
gh run watch "$(gh run list --workflow=release.yml --limit=1 --json databaseId --jq '.[0].databaseId')"

# 生成物のファイル名とサイズを確認（固定名になっているか）
RUN_ID=$(gh run list --workflow=release.yml --limit=1 --json databaseId --jq '.[0].databaseId')
# gh run download は artifact 名のディレクトリに展開されます
gh run download "$RUN_ID" -n floatsubs-windows-dist
gh run download "$RUN_ID" -n floatsubs-macos-dist
ls -lh floatsubs-windows-dist floatsubs-macos-dist
```

`verify` と `publish` が skipped、`build` の 2 ジョブが success で、上の 3 ファイルが
そろっていれば OK です（この時点の artifact は 7 日で消えます）。

### 2.3 public に切り替える（順番に注意）

ダウンロードリンク・バッジ・Pages は **公開直後から 404 になる期間** があります。避けたい場合は
2.3 より先に「2.7 初回リリース」を private のまま実行し、Release と 4 アセットができてから
public に切り替えてください（その場合、private リポジトリの Actions 分数を消費します。macOS は
10 倍換算です）。public を先にする場合は、続けてすぐタグを打って Release を作ってください。

```bash
gh repo edit sp-yduck/floatsubs \
  --visibility public \
  --accept-visibility-change-consequences
```

> **注意**: public にすると **これまでのコミット履歴がすべて公開** されます。private に戻すことは
> できますが、一度公開された内容は取り消せません。2.1 のチェックを先に済ませてください。

### 2.4 リポジトリの説明・トピック・Pages を設定する

```bash
gh repo edit sp-yduck/floatsubs \
  --description "PC の音声をリアルタイムで文字起こし・翻訳し、字幕としてオーバーレイ表示するオフライン・デスクトップアプリ（Tauri / whisper.cpp / PLaMo 3 Translate）" \
  --homepage "https://sp-yduck.github.io/floatsubs/" \
  --add-topic tauri --add-topic rust --add-topic whisper-cpp --add-topic llama-cpp \
  --add-topic plamo --add-topic speech-recognition --add-topic realtime-translation \
  --add-topic subtitles --add-topic overlay --add-topic desktop-app \
  --add-topic windows --add-topic macos --add-topic japanese

# GitHub Pages（main ブランチの /docs をそのまま配信）
gh api -X POST repos/sp-yduck/floatsubs/pages \
  --input - <<'JSON'
{"build_type":"legacy","source":{"branch":"main","path":"/docs"}}
JSON

# 反映確認（数十秒〜数分かかることがあります）
curl -sIL -o /dev/null -w '%{http_code}\n' https://sp-yduck.github.io/floatsubs/
```

GitHub の画面から設定する場合: **Settings → Pages → Build and deployment → Source =
"Deploy from a branch"、Branch = `main`、フォルダ = `/docs`**。`docs/.nojekyll` を置いてあるので
Jekyll の処理は入りません。

### 2.5 脆弱性の非公開報告を有効にする

[SECURITY.md](SECURITY.md) は GitHub の Private vulnerability reporting を前提にしています。

```bash
gh api -X PUT repos/sp-yduck/floatsubs/private-vulnerability-reporting
```

画面からは **Settings → Code security → Private vulnerability reporting** を Enable。

### 2.6 依存更新 PR 用のラベルを作る（任意）

`.github/dependabot.yml` は `dependencies` / `rust` / `github-actions` ラベルを付けます。
無い場合ラベルは付きませんが、Dependabot 自体は動作します。

```bash
gh label create dependencies    -R sp-yduck/floatsubs --color 0366d6 --description "Dependency updates"
gh label create rust            -R sp-yduck/floatsubs --color dea584 --description "Rust / Cargo"
gh label create github-actions  -R sp-yduck/floatsubs --color 2088ff --description "GitHub Actions"
```

### 2.7 初回リリース（v0.1.0）

```bash
git switch main && git pull
git tag -a v0.1.0 -m "floatsubs v0.1.0"
git push origin v0.1.0

# 進行確認（Windows / macOS のビルドで 30〜60 分ほどかかります）
gh run watch "$(gh run list --workflow=release.yml --limit=1 --json databaseId --jq '.[0].databaseId')"
```

### 2.8 公開後の確認

```bash
gh release view v0.1.0

# 「GitHub にログインしていない人」でもダウンロードできることを確認する
# （curl は認証ヘッダを付けないので、これがそのまま未ログイン検証になります）
for f in floatsubs-windows-x64-setup.exe floatsubs-windows-x64.msi \
         floatsubs-macos-arm64.dmg SHA256SUMS.txt; do
  printf '%s: ' "$f"
  curl -sIL -o /dev/null -w '%{http_code}\n' \
    "https://github.com/sp-yduck/floatsubs/releases/latest/download/$f"
done

# ランディングページ
curl -sIL -o /dev/null -w '%{http_code}\n' https://sp-yduck.github.io/floatsubs/
```

4 アセットすべて **200**、Pages も **200** なら公開完了です。README のバッジ
（CI / latest release）もこの時点から表示されます。

---

## 3. 2 回目以降のリリース

1. **バージョンを 3 箇所で上げる**（`verify` がこの 3 つの一致を要求します）
   - `src-tauri/tauri.conf.json` の `version`
   - `src-tauri/Cargo.toml` の `[package] version`
   - `crates/core/Cargo.toml` の `[package] version`
2. **`Cargo.lock` を更新**（`version` がロックにも入っているため）

   ```bash
   cargo metadata --no-deps --format-version 1 > /dev/null   # Cargo.lock を更新
   git diff --stat Cargo.lock
   ```
3. **`CHANGELOG.md` に `## [X.Y.Z] - YYYY-MM-DD` を追記**。Release 本文に自動で取り込まれます
   （見出しの書式 `## [X.Y.Z] - YYYY-MM-DD` を変えないこと）。
4. テストとビルド確認

   ```bash
   cargo test -p floatsubs-core
   gh workflow run release.yml --ref <ブランチ名>   # ドライラン（Release は作られない）
   ```

5. PR を作って `main` にマージしたあと、タグを push

   ```bash
   git switch main && git pull
   git tag -a vX.Y.Z -m "floatsubs vX.Y.Z"
   git push origin vX.Y.Z
   ```

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

### 同じバージョンで作り直したい

Release に 4 アセットがそろっていると、`verify` が「公開済み」と判断して **ビルドをスキップ** します
（二重公開・タグの取り違えを防ぐため）。作り直すときは Release とタグを消してから打ち直します。

```bash
gh release delete vX.Y.Z --yes
git push origin :refs/tags/vX.Y.Z
git tag -d vX.Y.Z
git tag -a vX.Y.Z -m "floatsubs vX.Y.Z"
git push origin vX.Y.Z
```

### タグを打ったのに何も起きない

- タグが `v0.1.0` 形式か（`0.1.0` や `release-0.1.0` は対象外）
- `release.yml` が `main` に存在するか（`workflow_dispatch` は default branch にファイルが
  無いと手動実行できません）
- コミットが `main` にマージ済みか

### アセット名や URL を変えたい

固定名は README / README.en.md / docs/index.html / Release 本文 / このドキュメントの契約です。
`release.yml` のステージング名・サイズ下限・`verify` の `release_exists` 期待値を同時に更新し、
利用者向けのリンクもすべて揃えてからタグを打ってください。

---

## 5. コード署名を追加する場合

現在の配布ビルドは **Windows のコード署名なし / macOS は ad-hoc 署名のみ（公証なし）** のため、
SmartScreen と Gatekeeper の警告が出ます（[公式サイトの「警告が表示されたとき」](https://sp-yduck.github.io/floatsubs/#warning)）。
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

署名を入れるまでは、公式サイトの「警告が表示されたとき」（<https://sp-yduck.github.io/floatsubs/#warning>）を案内してください。
