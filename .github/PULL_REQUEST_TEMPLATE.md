## 概要

<!-- この PR で何をしたかを 1〜3 行で書いてください。 -->

## 関連 Issue

<!-- 例: Closes #123 / Related to #456 -->

## 変更内容

<!-- 主な変更点を箇条書きで。レビューで見てほしい箇所があれば明記してください。 -->

-

## 動作確認

<!-- 実行したコマンドと結果を書いてください。 -->

- [ ] `cargo test -p overlaysubs-core` が通る
- [ ] `cargo build -p overlaysubs`（または `npx --yes @tauri-apps/cli@^2 dev`）で起動を確認した
- [ ] 手動で確認した操作:

```text
$ cargo test -p overlaysubs-core
```

## UI の変更

<!-- UI を変更した場合、Before / After のスクリーンショットや説明を添付してください。なければ「なし」と書いてください。 -->

- Before:
- After:

## チェックリスト

- [ ] 秘密情報（API キー、トークン、個人情報、ローカルの絶対パス）を含めていない
- [ ] UI の文言を変更した場合、`ui/i18n.js` の日本語（`ja`）と英語（`en`）の両方を更新した
- [ ] 動作・インストール手順・必要環境が変わる場合、README.md（英語）/ README.ja.md（日本語）/ docs/index.html / docs/en/index.html を更新した
- [ ] `models.json` を変更した場合、コミット固定 URL・サイズ・SHA-256 を実ファイルと一致させた（`/resolve/main/` を使っていない）
- [ ] ライセンスや利用規約（`licenses/`、`Built with PLaMo` 表記、同意フロー）に影響しない。影響する場合は理由を上に書いた
- [ ] 大きな変更の場合、事前に Issue で方針を相談した
- [ ] `cargo fmt` を実行した（推奨。CI では強制していません）
