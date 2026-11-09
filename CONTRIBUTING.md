# Contributing to OverlaySubs

**English** · [日本語](CONTRIBUTING.ja.md)

Thanks for contributing to OverlaySubs. Bug reports, feature requests, translation improvements, and documentation fixes are all welcome — you do not have to write code.

This project follows the [Contributor Covenant 2.1](CODE_OF_CONDUCT.md). Please read it before you participate.

- For large changes, open an issue to discuss the approach before you start (to agree on the design and avoid duplicate work).
- If you find a security issue, **do not open a public issue**; report it privately following [SECURITY.md](SECURITY.md).
- Contributions are provided under the [MIT License](LICENSE). You do not need to sign a CLA.

## Language

English is the default language of this repository. Please write issues, pull requests, and commit messages in English.

Japanese is kept as a secondary language for user-facing material (`README.ja.md`, the Japanese pages in `docs/`, the Japanese terms of use, and the Japanese UI strings in `ui/i18n.js`). When you change user-facing text, update the English text first and the Japanese translation alongside it.

Do not translate functional data or legal texts: the terms of use (`licenses/`), the hallucination phrases and language-specific punctuation rules in `crates/core`, and model or file identifiers must stay as they are.

## Development environment

### Common

- **Rust stable** (edition 2021). Install it with `rustup`.
- **Git**. On Windows, run `git config --global core.longpaths true` (the llama.cpp submodule paths exceed Windows' MAX_PATH).
- **Node.js 20 or later**. It is not needed by the app itself; it is only used to run the Tauri CLI through `npx`.
- The first build compiles whisper.cpp and llama.cpp (ggml) from C/C++, which takes **10–20 minutes** and grows `target/` to several GB. Incremental builds after that take seconds to a few tens of seconds.

### Windows

- **Visual Studio 2022 Build Tools** with the "Desktop development with C++" workload (MSVC + Windows SDK).
- **CMake** and **LLVM / libclang** (whisper-rs and llama-cpp-sys generate bindings with bindgen).
- **WebView2 Runtime** (normally preinstalled on Windows 10/11).
- **Vulkan SDK** (required). CI installs `1.4.313.2` from LunarG's installer. Set `VULKAN_SDK` and add `%VULKAN_SDK%\Bin` to `PATH`.
- If you hit "path too long" errors, point `CARGO_TARGET_DIR` at a short path (CI uses `C:\t`).

```powershell
$env:CARGO_TARGET_DIR = "C:\t"
```

### macOS

- **Xcode Command Line Tools** (`xcode-select --install`).
- Apple Silicon (M1 or later) and macOS 13 or later. Build with `--features metal`.
- When creating a `.dmg`, `scripts/stage-dylibs.sh` is invoked from `beforeBundleCommand` and stages the llama.cpp dylibs.

### Linux

Not a distribution target (development builds may work but are unsupported). For Linux-specific pull requests, discuss the approach in an issue first.

## Tests

Core tests that need no network (these are what CI runs on pull requests).

```bash
cargo test -p overlaysubs-core
```

`overlaysubs-core` covers tier selection, model definitions, PFML3 prompts, utterance segmentation, sessions, and SRT export.

The end-to-end tests that use the real models are `ignore`d by default. They run if you provide a 16 kHz mono WAV (they download about 0.7 GB of models).

```bash
OVERLAYSUBS_WAV=path/to/16k-mono.wav cargo test -p overlaysubs -- --ignored
```

Depending on what you change, run these tests too.

```bash
cargo test -p overlaysubs          # unit tests in src-tauri
cargo build -p overlaysubs         # check that it compiles
```

## Build and run

During development the fastest way to start is Tauri's dev mode. Add GPU features as needed.

```bash
# settings window + overlay
npx --yes @tauri-apps/cli@^2 dev --features vulkan   # Windows
npx --yes @tauri-apps/cli@^2 dev --features metal    # macOS
```

To build release bundles (`.exe` / `.msi` / `.dmg`):

```bash
cd src-tauri
npx --yes @tauri-apps/cli@^2 build --features vulkan   # Windows
npx --yes @tauri-apps/cli@^2 build --features metal    # macOS
```

You can also `cargo install tauri-cli --version "^2" --locked` and use `cargo tauri build`. For NVIDIA GPUs, use `--features cuda`. Releases are published only through GitHub Releases.

## Code layout

| Path | Contents |
|---|---|
| `crates/core/` | Engine-independent logic: tier selection (`tier.rs`), model definitions (`registry.rs`), PFML3 prompts (`pfml.rs`), utterance segmentation (`endpoint.rs`), sessions (`session.rs`), SRT/TXT export (`export.rs`) |
| `src-tauri/src/` | The Tauri app: `main.rs` (settings, commands, tray, hotkeys), `capture.rs` (WASAPI loopback / microphone), `engine.rs` (whisper.cpp / llama.cpp adapters), `download.rs` (resumable downloads with SHA-256 verification), `hw.rs` (hardware detection) |
| `src-tauri/capabilities/` | Tauri permissions. Update this when you use a new API. |
| `ui/` | Settings window and overlay: plain HTML/JS/CSS with no build step. The strings live in `ui/i18n.js` (Japanese and English). |
| `models.json` | Model definitions (URL, size, SHA-256, tier). |
| `licenses/` | OverlaySubs Terms of Use (Japanese and English). The PLaMo 3 license is linked, not bundled. |
| `scripts/` | Helper scripts for releases and bundling (`version.sh` prints the version, `set-version.sh` updates it). |
| `docs/` | End-user documentation (the GitHub Pages source: English `docs/index.html`, Japanese `docs/ja/index.html`, shared CSS `docs/style.css`; `docs/en/index.html` is a redirect stub for old links). Installation, usage, FAQ, and licenses are collected here. Keep the English page self-contained; the Japanese page currently loads Google Fonts. |

## Changing model definitions

`models.json` directly affects the app's behaviour and the integrity of downloads.

- Pin URLs to a **Hugging Face commit**. Do not use `/resolve/main/` (an upstream re-upload would break the hash). A test in `crates/core` enforces this rule.
- Make `size` and `sha256` match the actual files. Downloads delete the partial file when hash verification fails.
- Every tier needs a default ASR and translation model (VAD is shared by all tiers). `Registry::parse` validates this at startup.
- If you change a size, also update the download-size tables in `docs/index.html` and `docs/ja/index.html` (for users) and the tier table in the READMEs.

## UI and language

- `ui/i18n.js` contains English (`en`) and Japanese (`ja`). When you add or change UI text, update **both**.
- When you add a setting, update the matching page in `ui/app.js` and, if needed, `Settings` in `src-tauri/src/main.rs`.
- Do not remove the overlay credit (`Built with PLaMo`), the terms of use, or the link to the PLaMo 3 license; they have licensing significance.

## Commit messages

Write commit messages in English, subject and body. Keep the subject short and imperative, and explain why in the body when needed.

```
Add Silero VAD-based utterance segmentation
Make model downloads resumable

Fix Windows bundle paths exceeding MAX_PATH
```

For large changes, reference the related issue in the body (e.g. `#123`).

Pull requests are squash-merged, and the pull request title becomes the entry in the release notes. Do not update any changelog file: the change list is generated from the merged pull request titles.

The existing history is in Japanese, the project's original working language. Do not rewrite it; English is the default from here on.

## Pull requests

1. Fork the repository and create a working branch from `main` (e.g. `feat/silero-vad`, `fix/windows-paths`).
2. Commit your changes.
3. Open a pull request against `main` with an English title (it becomes the release note entry). Fill in the checklist in the template.
4. **CI (`cargo test -p overlaysubs-core`) must pass.**
5. Address the review comments.

Changes that touch `src-tauri/`, native dependencies (whisper.cpp / llama.cpp), or bundle configuration may prompt a maintainer to add the **`ci:build` label** and run the Windows / macOS release build (the `build` job in `.github/workflows/release.yml`). It also works for pull requests from forks, and the installers are kept as artifacts of that run, so you can install and test them on real hardware before merging (`verify` / `publish` are skipped and no release is created). If you want a build, say so in the pull request description.

To try it in your own fork first, you can run the same dry run there (the artifacts stay in your fork and are not visible from the upstream repository).

```bash
gh workflow run release.yml --repo <your-user-name>/overlaysubs --ref <working-branch>
```

Before opening a pull request, check the following.

- `cargo test -p overlaysubs-core` passes.
- No secrets (API keys, tokens, personal information, local paths) are included.
- If you changed UI text, you updated both English and Japanese in `ui/i18n.js`.
- If behaviour or installation steps change, you updated the end-user docs (`docs/index.html` / `docs/ja/index.html`) and `README.md` (English) / `README.ja.md` (Japanese) (for developers).
- If your change affects licenses or the terms of use (`licenses/`, the link to the PLaMo 3 license), you explained why in the pull request.
- There is no changelog file to update: the release notes are generated from the pull request titles.

### Formatting

- Rust: `cargo fmt` is recommended (**not yet enforced in CI**). Fixing `cargo clippy` warnings is welcome too.
- Indentation: 4 spaces for Rust, 2 spaces for JS / HTML / CSS. Line endings are LF and the encoding is UTF-8 (see [.editorconfig](.editorconfig)).
- Markdown may keep trailing whitespace (the [.editorconfig](.editorconfig) sets `trim_trailing_whitespace = false`).

### Code of conduct

All participants must follow [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

### Security

Do not report vulnerabilities in a public issue; report them privately following [SECURITY.md](SECURITY.md).
