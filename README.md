# OverlaySubs

**English** · [日本語](README.ja.md)

[![CI](https://github.com/sp-yduck/overlaysubs/actions/workflows/ci.yml/badge.svg)](https://github.com/sp-yduck/overlaysubs/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/sp-yduck/overlaysubs?label=release&sort=semver)](https://github.com/sp-yduck/overlaysubs/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Windows x64](https://img.shields.io/badge/platform-Windows%20x64-0078d4.svg)](#requirements)
[![macOS Apple Silicon](https://img.shields.io/badge/platform-macOS%20Apple%20Silicon-lightgrey.svg)](#requirements)

A desktop app that transcribes and translates any audio playing on your PC in real time and shows the result as an overlay of subtitles. Audio and text are processed locally on your device — no API key is needed, and nothing is sent to an external service.

**Built with PLaMo.** Translation uses Preferred Networks' [PLaMo 3 Translate](https://huggingface.co/pfnet/plamo-3-translate-0.8b-GGUF).

## Screenshots

**Settings** — translation options, languages and recent subtitles.
![Settings screen](docs/images/ui-settings-en.png)

**Model setup** — on first run the app detects your hardware, picks a tier and downloads the matching models.
![Model setup screen](docs/images/ui-model-setup-en.png)

**Subtitle overlay** — drawn on top of other apps; click-through is available in the settings (background is illustrative).
![Subtitle overlay](docs/images/overlay.jpg)

## For users (download and install)

**You do not need a GitHub account.** If you just want to use the app, follow the instructions on the official site.

- Official site (download, install, usage, FAQ, uninstall, license): <https://sp-yduck.github.io/overlaysubs/>
- Windows 10 / 11 (x64): [overlaysubs-windows-x64-setup.exe](https://github.com/sp-yduck/overlaysubs/releases/latest/download/overlaysubs-windows-x64-setup.exe) (recommended)
- Windows (MSI): [overlaysubs-windows-x64.msi](https://github.com/sp-yduck/overlaysubs/releases/latest/download/overlaysubs-windows-x64.msi) (installs per-machine and asks for Administrator; the `.exe` is recommended)
- macOS 13+ (Apple Silicon): [overlaysubs-macos-arm64.dmg](https://github.com/sp-yduck/overlaysubs/releases/latest/download/overlaysubs-macos-arm64.dmg) (no Intel Mac build)
- Checksums: [SHA256SUMS.txt](https://github.com/sp-yduck/overlaysubs/releases/latest/download/SHA256SUMS.txt) / releases: <https://github.com/sp-yduck/overlaysubs/releases/latest>

Install steps, what to do when Windows SmartScreen or macOS Gatekeeper warns you, SHA-256 verification, usage, the FAQ, and uninstalling are all on the official site:
[Install](https://sp-yduck.github.io/overlaysubs/#install) / [Warnings](https://sp-yduck.github.io/overlaysubs/#warning) / [Usage](https://sp-yduck.github.io/overlaysubs/#usage) / [FAQ](https://sp-yduck.github.io/overlaysubs/#faq) / [Uninstall](https://sp-yduck.github.io/overlaysubs/#uninstall) / [License and terms](https://sp-yduck.github.io/overlaysubs/#license)

> The builds are **not signed with an Apple Developer ID or a Windows code-signing certificate, and not notarized** (the macOS app is ad-hoc signed only), so SmartScreen / Gatekeeper warn on first launch. That does not mean the app is unsafe. See [Warnings](https://sp-yduck.github.io/overlaysubs/#warning) on the site, and never disable Gatekeeper or SmartScreen system-wide.

## Contents

- [Screenshots](#screenshots)
- [Requirements](#requirements)
- [Architecture](#architecture)
- [Features](#features)
- [Models and tiers](#models-and-tiers)
- [Build](#build)
- [Tests](#tests)
- [Contributing](#contributing)
- [License](#license)

## Requirements

| | Windows | macOS |
|---|---|---|
| OS | Windows 10 / 11 (64-bit) | macOS 13 Ventura or later |
| CPU | x64 (Intel / AMD) | Apple Silicon (M1 or later) |
| GPU | **Vulkan-capable GPU with an up-to-date driver** | Integrated GPU (Metal) |
| Memory | 8 GB or more | 8 GB or more |
| Free disk space | Model download size (see [Models and tiers](#models-and-tiers)) | Same |

- Windows builds require a **Vulkan-capable GPU and current driver** (the official build uses `--features vulkan`). With an old driver the GPU is not used and performance is not usable in real time.
- There is no Intel Mac build, no 32-bit Windows build, and no Windows on ARM build.
- The only supported distribution channel is **GitHub Releases**; there is no auto-updater.

## Architecture

```
WASAPI loopback / microphone ─▶ 16 kHz mono ─▶ Silero VAD endpointer ─▶ whisper.cpp ─▶ PLaMo 3 Translate (llama.cpp) ─▶ overlay
```

- `crates/core` holds engine-independent logic: tier selection, model definitions, PFML3 prompts, utterance segmentation, sessions, sentence-level translation, and SRT export. Run `cargo test -p overlaysubs-core`.
- `src-tauri` is the Tauri app: audio capture, whisper.cpp / llama.cpp adapters, model downloads, tray, hotkeys, and settings persistence.
- `ui` is the settings window and overlay: plain HTML/JS/CSS with no build step; `ui/i18n.js` holds the Japanese and English strings.
- The ASR and translation engines sit behind the `session::Asr` / `session::Translator` traits and can be swapped.
- `models.json` defines the models (URL, size, SHA-256, tier). URLs are pinned to Hugging Face commits (using `main` would break the hash when upstream re-uploads). Placing a `models.json` in the config folder overrides the bundled one without an app update.
- `licenses/` ships the OverlaySubs Terms of Use and the PLaMo Community License (Japanese and English); `THIRD_PARTY_NOTICES.md` lists the bundled libraries' licenses.

## Features

- First run asks you to accept the OverlaySubs Terms of Use (with the PLaMo Community License as an appendix). Acceptance is stored in `terms-accepted` and not asked again until the terms version changes. "Check this PC" in the settings window detects your hardware and picks a tier (High / Mid / Low / CPU).
- Models download from Hugging Face automatically, resume after interruption, and are verified with SHA-256. No account or token required.
- You can run the optional speed check (benchmark); if it cannot keep up in real time, it switches to one tier lower.
- Audio input is Windows WASAPI loopback (system audio) or a microphone. On macOS you pick a loopback input device as the microphone.
- Source language auto-detect or fixed. You can pick the target language and the "if already in that language" language, so English↔Japanese and Korean↔Japanese work together.
- Previous utterances are passed as context and translation happens per sentence. Glossary, a free-text topic hint, and Japanese plain/polite style are configurable.
- Partial results are dimmed; final translations stream in and replace them.
- Always-on-top, semi-transparent overlay (click-through can be enabled in settings). Original text, font size, font, background opacity, line count, display time, and the "Built with PLaMo" credit are configurable.
- Export history as text or SRT.
- Tray/menu-bar resident, global hotkey (default `CmdOrCtrl+Shift+T`; `Ctrl+Shift+T` on Windows, `Cmd+Shift+T` on macOS) to start/stop. UI in Japanese and English.
- Audio, transcripts, and translations are never sent anywhere. The only network traffic is the model download. No telemetry. The app works offline once the models are downloaded.

## Models and tiers

The tier is decided when you run "Check this PC" in first-run setup, and the models for that tier are selected automatically. The settings window lets you change the individual Whisper / PLaMo / VAD models.

| Tier | Detected when | Whisper | PLaMo 3 Translate | First download |
|---|---|---|---|---|
| High | VRAM 12 GB+ / Apple Silicon RAM 32 GB+ | large-v3-turbo | 8B Q4_K_M | ~6.9 GB |
| Mid | VRAM 8 GB / Apple Silicon RAM 16 GB | large-v3-turbo Q5_0 | 2B Q4_K_M | ~2.4 GB |
| Low | VRAM 4–6 GB / Apple Silicon RAM 8 GB | small | 2B Q4_K_M | ~2.3 GB |
| CPU | No GPU, or VRAM under 4 GB | base | 0.8B Q4_K_M | ~0.7 GB |

- Sizes are the sum of the declared sizes in `models.json` (including the ~0.9 MB Silero VAD). Allow about 10 GB free for High, 5 GB for Mid/Low, and 2 GB for CPU.
- Mid uses 2B instead of 8B because keeping Whisper and 8B Q4_K_M (5.3 GB) together in 8 GB of VRAM leaves no headroom for gaming. You can switch to 8B Q3_K_M or 31B manually in the settings window.
- The benchmark is optional. If you run it and the machine cannot keep up in real time, it suggests one tier lower and can switch to it.

## Build

Prerequisites: Rust stable (edition 2021). On Windows, MSVC + Windows SDK, CMake, LLVM / libclang (for bindgen), the Vulkan SDK, and WebView2. On macOS, Xcode Command Line Tools. Node.js is only needed to run the Tauri CLI through `npx` (not for the app itself).

```bash
# Windows (Vulkan, same as the official build)
npx --yes @tauri-apps/cli@2.12.4 build --features vulkan

# macOS (Metal)
npx --yes @tauri-apps/cli@2.12.4 build --features metal

# NVIDIA
npx --yes @tauri-apps/cli@2.12.4 build --features cuda
```

You can also `cargo install tauri-cli --version "^2" --locked` and use `cargo tauri build`. CI uses the prebuilt `npx @tauri-apps/cli@2.12.4` because `cargo install tauri-cli` took up to 19 minutes.

whisper.cpp and llama.cpp both embed ggml, so statically linking both into one process clashes on symbols. llama.cpp is therefore linked as a shared library (`dynamic-link` in `llama-cpp-2`) and shipped inside the installer.

- Windows: `src-tauri/libs/*.dll` is bundled via `bundle.resources`. `src-tauri/libs/` is gitignored, so before bundling copy llama.cpp's build output (`target/release/build/llama-cpp-sys-2-*/out/bin/*.dll`) into `src-tauri/libs/`. The CI steps are in `.github/workflows/release.yml`.
- macOS: `beforeBundleCommand` runs `scripts/stage-dylibs.sh`, which collects the dylibs into `target/frameworks`; `bundle.macOS.files` ships them in `Frameworks`.
- `THIRD_PARTY_NOTICES.md` is bundled too (see `src-tauri/tauri.windows.conf.json` and `tauri.macos.conf.json`).

## Tests

The network-free core tests run in CI (`.github/workflows/ci.yml`) on pull requests.

```bash
cargo test -p overlaysubs-core   # tiers, model registry, PFML3, endpointer, session, SRT
cargo test -p overlaysubs        # src-tauri unit tests
```

The end-to-end test with real models is `#[ignore]`d by default (it needs a 16 kHz mono WAV and downloads ~0.7 GB of models).

```bash
OVERLAYSUBS_WAV=path/to/16k-mono.wav cargo test -p overlaysubs -- --ignored
```

Pushing a `v*` tag makes the release workflow build the bundles and publish them to GitHub Releases; see [RELEASING.md](RELEASING.md).

## Contributing

- [CONTRIBUTING.md](CONTRIBUTING.md) — dev environment, tests, PR flow
- [SECURITY.md](SECURITY.md) — how to report vulnerabilities privately
- [CHANGELOG.md](CHANGELOG.md) — release history
- [RELEASING.md](RELEASING.md) — release process (maintainers)
- [docs/index.html](docs/index.html) — end-user documentation (the GitHub Pages source; self-contained, no external CDN, fonts, or JS)
- Issue templates: [bug report](.github/ISSUE_TEMPLATE/bug_report.yml) / [feature request](.github/ISSUE_TEMPLATE/feature_request.yml)

## License

- **The app code is MIT licensed** ([LICENSE](LICENSE)).
- Use of the app is governed by the [OverlaySubs Terms of Use](licenses/overlaysubs-terms.ja.txt) ([English](licenses/overlaysubs-terms.en.txt)). The terms include the [PLaMo Community License](https://plamo.preferredai.jp/info/plamo-community-license) as an appendix, accepted together on first launch. The full texts ship in [`licenses/`](licenses/) and are also available from "About" in the app.
  - Personal and non-commercial use is free.
  - Commercial use requires prior registration. Business use by you or your affiliates with annual revenue over JPY 1 billion requires a separate agreement with Preferred Networks.
  - If you publish or distribute subtitles or other output, state that it is PLaMo output. Exported text/SRT does not include the credit, so state it yourself when publishing. The subtitle overlay shows `Built with PLaMo` by default (can be turned off in settings).
- If you publish subtitles for streams or videos, mind the copyright of the source content.
- Whisper models (MIT) and Silero VAD (MIT) follow their own licenses.
- Third-party licenses for the bundled whisper.cpp / llama.cpp / ggml are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
