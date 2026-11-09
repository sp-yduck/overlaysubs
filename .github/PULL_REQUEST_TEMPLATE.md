## Summary

<!-- Describe what this pull request does in 1–3 lines. -->

## Related issue

<!-- e.g. Closes #123 / Related to #456 -->

## Changes

<!-- List the main changes. Call out anything the reviewer should look at closely. -->

-

## Testing

<!-- Describe the commands you ran and their results. -->

- [ ] `cargo test -p overlaysubs-core` passes
- [ ] I confirmed that it starts with `cargo build -p overlaysubs` (or `npx --yes @tauri-apps/cli@^2 dev`)
- [ ] If an installer build is needed, I asked for one with the `ci:build` label in the pull request description
- [ ] Manual checks I performed:

```text
$ cargo test -p overlaysubs-core
```

## UI changes

<!-- If you changed the UI, attach before / after screenshots or a description. If not, write "None". -->

- Before:
- After:

## Checklist

- [ ] No secrets (API keys, tokens, personal information, local absolute paths) are included
- [ ] If I changed UI text, I updated both English (`en`) and Japanese (`ja`) in `ui/i18n.js`
- [ ] If behaviour, installation steps, or requirements change, I updated README.md (English) / README.ja.md (Japanese) / docs/index.html / docs/en/index.html
- [ ] If I changed `models.json`, the pinned commit URLs, sizes, and SHA-256 values match the actual files (no `/resolve/main/`)
- [ ] This does not affect licenses or the terms of use (`licenses/`, the link to the PLaMo 3 license, the `Built with PLaMo` credit, the consent flow). If it does, I explained why above
- [ ] For large changes, I discussed the approach in an issue beforehand
- [ ] I ran `cargo fmt` (recommended; CI does not enforce it)
