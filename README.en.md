# floatsubs

[![CI](https://github.com/sp-yduck/floatsubs/actions/workflows/ci.yml/badge.svg)](https://github.com/sp-yduck/floatsubs/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/sp-yduck/floatsubs?label=release&sort=semver)](https://github.com/sp-yduck/floatsubs/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Windows x64](https://img.shields.io/badge/platform-Windows%20x64-0078d4.svg)](#requirements)
[![macOS Apple Silicon](https://img.shields.io/badge/platform-macOS%20Apple%20Silicon-lightgrey.svg)](#requirements)

A desktop app that transcribes and translates any audio playing on your PC in real time and shows the result as an overlay of subtitles.
**Install it, launch it, done. No API key. Free.**

**Built with PLaMo.** Translation uses Preferred Networks' [PLaMo 3 Translate](https://huggingface.co/pfnet/plamo-3-translate-0.8b-GGUF).

## Download

**You do not need a GitHub account.** Click a link below, download the file, and run it. No git or Rust knowledge required.
(There is also a [landing page](https://sp-yduck.github.io/floatsubs/).)

| Your PC | Download | Notes |
|---|---|---|
| **Windows 10 / 11 (x64)** | [**floatsubs-windows-x64-setup.exe**](https://github.com/sp-yduck/floatsubs/releases/latest/download/floatsubs-windows-x64-setup.exe) | Recommended installer |
| Windows (MSI) | [floatsubs-windows-x64.msi](https://github.com/sp-yduck/floatsubs/releases/latest/download/floatsubs-windows-x64.msi) | Alternative if you need an MSI |
| **macOS 13+ (Apple Silicon)** | [**floatsubs-macos-arm64.dmg**](https://github.com/sp-yduck/floatsubs/releases/latest/download/floatsubs-macos-arm64.dmg) | M1 / M2 / M3 / M4. No Intel Mac build |

- Release page: <https://github.com/sp-yduck/floatsubs/releases/latest>
- All releases (including older versions): <https://github.com/sp-yduck/floatsubs/releases>
- Checksums: [SHA256SUMS.txt](https://github.com/sp-yduck/floatsubs/releases/latest/download/SHA256SUMS.txt)

> The MSI installs per-machine and therefore asks for Administrator (UAC). The `.exe` installer is per-user and is the recommended option.

### Install in 3 steps

1. **Download** — click the file for your PC in the table above.
2. **Run** — open the downloaded file. On Windows run the `.exe`; on macOS open the `.dmg` and drag floatsubs into Applications.
3. **First-run setup** — launch the app, accept the Terms of Use, then "Check this PC" → "Download models" (takes a few to about 15 minutes).

Models are downloaded only once. After that you can start translating immediately.

### If Windows SmartScreen or macOS Gatekeeper warns you

The builds are **not signed with an Apple Developer ID or Windows code-signing certificate, and not notarized** (the macOS app is ad-hoc signed only), so the OS shows a warning on first launch. That does **not** mean the app is unsafe. Use the steps below.

**Windows — "Windows protected your PC"**

1. Click **More info** on the blue dialog.
2. Click **Run anyway**.

**macOS — "cannot be opened because the developer cannot be verified" / "not opened"**

1. In Finder, **right-click (or Control-click)** floatsubs → **Open** → **Open** in the dialog.
2. If it still will not open (macOS 15 and later), open **System Settings → Privacy & Security** and click **Open Anyway** next to the message about floatsubs.

> [!WARNING]
> Do not disable SmartScreen or Gatekeeper system-wide. Only allow this one app, using the steps above. macOS 15 removed the Control-click bypass, so the System Settings route is required there.

### Requirements

| | Windows | macOS |
|---|---|---|
| OS | Windows 10 / 11 (64-bit) | macOS 13 Ventura or later |
| CPU | x64 (Intel / AMD) | Apple Silicon (M1 or later) |
| GPU | **Vulkan-capable GPU with an up-to-date driver** | Integrated GPU (Metal) |
| Memory | 8 GB or more | 8 GB or more |
| Free disk space | See the model table below | Same |

- Windows builds require a Vulkan-capable GPU and current driver. With an old driver the GPU is not used and performance is not usable in real time. Install the latest driver from NVIDIA, AMD, or Intel.
- There is no Intel Mac build, no 32-bit Windows build, and no Windows on ARM build.
- In first-run setup, "Check this PC" reads GPU VRAM (memory on Apple Silicon), decides the High / Mid / Low / CPU tier, and selects models for that tier. See [Tiers](#tiers).
- The only supported distribution channel is **GitHub Releases**; there is no auto-updater.

### First-run model download (several GB)

On first launch the app picks models for your hardware and downloads them from Hugging Face.
**No Hugging Face account or token is needed.** Downloads resume after interruption and are verified with SHA-256.

| Tier | Automatically selected | Download size | Suggested free space |
|---|---|---|---|
| High | Whisper large-v3-turbo + PLaMo 3 Translate 8B (Q4_K_M) | ~6.9 GB | 10 GB+ |
| Mid | Whisper large-v3-turbo (Q5_0) + PLaMo 3 Translate 2B (Q4_K_M) | ~2.4 GB | 5 GB+ |
| Low | Whisper small + PLaMo 3 Translate 2B (Q4_K_M) | ~2.3 GB | 5 GB+ |
| CPU | Whisper base + PLaMo 3 Translate 0.8B (Q4_K_M) | ~0.7 GB | 2 GB+ |

Sizes are the sum of the declared sizes in `models.json` (including the ~0.9 MB Silero VAD). Running "Check this PC" in first-run setup decides the tier from your hardware and selects the matching models; the settings window lets you change the individual Whisper / PLaMo models.

### Verify SHA-256 (optional)

Compare the output with [SHA256SUMS.txt](https://github.com/sp-yduck/floatsubs/releases/latest/download/SHA256SUMS.txt).

**Windows (PowerShell)**

```powershell
Get-FileHash .\floatsubs-windows-x64-setup.exe -Algorithm SHA256
```

**macOS (Terminal)**

```bash
shasum -a 256 floatsubs-macos-arm64.dmg
```

### Uninstall

**Windows**: Settings → Apps → Installed apps → floatsubs → Uninstall

**macOS**: drag floatsubs from Applications to the Trash

Downloaded models and settings remain in the folders below (so a reinstall does not re-download them). Delete them manually to reclaim the space.

| OS | Location |
|---|---|
| Windows | `%APPDATA%\io.github.floatsubs` |
| macOS | `~/Library/Application Support/io.github.floatsubs` |

The app identifier is `io.github.floatsubs`. Models live in `models`, settings in `settings.json`, and Terms acceptance in `terms-accepted`.

## Usage

1. On first launch, accept the Terms of Use (which include the PLaMo Community License as an appendix) and set up models.
2. Choose the audio input. On Windows, "System audio" captures everything your PC plays. On macOS, choose a loopback (virtual audio) device as the Microphone input (see [Known limitations](#known-limitations)).
3. **`Ctrl+Shift+T` (`Cmd+Shift+T` on macOS)** starts and stops translation. The hotkey is configurable.
4. Subtitles appear in a semi-transparent, always-on-top overlay. Click-through (so mouse input reaches the app underneath) can be enabled under "Subtitles", where you can also adjust position and looks.
5. Quit from the tray/menu-bar icon → "Quit". Closing the settings window also quits the app.

Audio, transcripts, and translations never leave your device. The only network access is the model download. There is no telemetry, and the app works offline once the models are downloaded.

## How it works

```
WASAPI loopback / microphone ─▶ 16 kHz mono ─▶ energy endpointer ─▶ whisper.cpp (+ Silero VAD) ─▶ PLaMo 3 Translate (llama.cpp) ─▶ overlay
```

- `crates/core` holds engine-independent logic: tier selection, model definitions, PFML3 prompts, utterance segmentation, sessions, and SRT export. Run `cargo test -p floatsubs-core`.
- `src-tauri` is the Tauri app: audio capture, whisper.cpp / llama.cpp adapters, downloads, tray, and hotkeys.
- `ui` is the settings window and overlay, plain HTML/JS with no build step.
- The ASR and translation engines sit behind the `session::Asr` / `session::Translator` traits and can be swapped.
- `models.json` defines the models (URL, size, SHA-256, tier). URLs are pinned to Hugging Face commits (using `main` would break the hash when upstream re-uploads). Placing a `models.json` in the config folder overrides the bundled one without an app update.

## Features

- First run asks you to accept the floatsubs Terms of Use (with the PLaMo Community License as an appendix). Acceptance is stored in `terms-accepted` and not asked again until the terms version changes. The app then detects your hardware and picks a tier (High / Mid / Low / CPU).
- Models download from Hugging Face automatically, resume after interruption, and are verified with SHA-256. No account or token required.
- You can run the optional speed check (benchmark) in the settings window; if it cannot keep up in real time, it switches to one tier lower.
- Source language auto-detect or fixed. You can pick the target language and the "if already in that language" language, so English↔Japanese and Korean↔Japanese work together.
- The previous three utterances are passed as context. Glossary and Japanese plain/polite style are configurable.
- Partial results are dimmed; final translations stream in and replace them.
- Always-on-top, semi-transparent overlay (click-through can be enabled in settings). Original text, font size, font, background opacity, line count, display time, and the "Built with PLaMo" credit are configurable.
- Export history as text or SRT.
- Tray/menu-bar resident, global hotkey (default `CmdOrCtrl+Shift+T`; `Ctrl+Shift+T` on Windows, `Cmd+Shift+T` on macOS) to start/stop. UI in Japanese and English.
- Audio, transcripts, and translations are never sent anywhere. The only network traffic is the model download. No telemetry.

## Tiers

The tier is decided when you run "Check this PC" in first-run setup, and the models for that tier are selected automatically.

| Tier | Detected when | Whisper | PLaMo 3 Translate |
|---|---|---|---|
| High | VRAM 12 GB+ / Apple Silicon RAM 32 GB+ | large-v3-turbo | 8B Q4_K_M |
| Mid | VRAM 8 GB / Apple Silicon RAM 16 GB | large-v3-turbo Q5_0 | 2B Q4_K_M |
| Low | VRAM 4–6 GB / Apple Silicon RAM 8 GB | small | 2B Q4_K_M |
| CPU | No GPU, or VRAM under 4 GB | base | 0.8B Q4_K_M |

Mid uses 2B instead of 8B because keeping Whisper and 8B Q4_K_M (5.3 GB) together in 8 GB of VRAM leaves no headroom for gaming. You can switch to 8B Q3_K_M or 31B manually in the settings window.

## Troubleshooting (FAQ)

**Windows shows "Windows protected your PC" and will not run.**
The build is unsigned, so this is expected. Click "More info" → "Run anyway" (see [the warning section](#if-windows-smartscreen-or-macos-gatekeeper-warns-you)).

**macOS says the developer cannot be verified.**
Right-click → Open. If that fails, use System Settings → Privacy & Security → "Open Anyway" (see [the warning section](#if-windows-smartscreen-or-macos-gatekeeper-warns-you)).

**The app launches but translation will not start.**
First-run setup is incomplete. Go to "Set up models" and work through "Check this PC" → "Download models". The start button stays disabled until the models are ready.

**The hotkey does nothing.**
Another app probably owns the same shortcut. Change `CmdOrCtrl+Shift+T` in the settings window.

**The GPU is not used / it is too slow.**
Windows needs a Vulkan-capable GPU and the latest driver. If it is still slow, pick lighter Whisper / PLaMo models in the settings window, or run "Measure speed" to switch to one tier lower. The CPU models work but are slower.

**macOS cannot capture system audio.**
ScreenCaptureKit capture is not implemented yet. Use a loopback (virtual audio) device such as BlackHole, select "Microphone" as the source, and pick that device (see [Known limitations](#known-limitations)).

**Model download fails.**
Re-run the download to resume. Check free disk space (see [the model table](#first-run-model-download-several-gb)). A partial file failing verification is deleted automatically and can be downloaded again.

**Does it work offline?**
Yes. Once the models are downloaded, transcription and translation work without a network connection.

**Can I use it commercially?**
The app code is MIT. The PLaMo model follows the PLaMo Community License: free for personal and non-commercial use, prior registration with PFN required for commercial use, and a separate agreement for business use by organizations with annual revenue over JPY 1 billion (see [License](#license)).

**May I publish subtitles in videos or streams?**
State "Built with PLaMo" when you publish or distribute output. Exported text/SRT does not include the credit, so add it yourself. Also mind the copyright of the source content.

**I uninstalled it but the disk is still full.**
Models and settings remain in the folders above; delete them manually (see [Uninstall](#uninstall)).

## Known limitations

- Builds are **not signed with an Apple Developer ID or a Windows code-signing certificate, and not notarized** (the macOS app is ad-hoc signed only), so SmartScreen / Gatekeeper warnings appear.
- No Intel Mac build (macOS is Apple Silicon only, macOS 13+).
- macOS system audio capture (ScreenCaptureKit) is not implemented. On macOS, pick a loopback input device as the microphone.
- Capturing only one app, and capturing system audio and microphone at the same time, are not implemented.
- There is no auto-update. Download new versions from GitHub Releases.
- GitHub Releases is the only supported distribution channel.

## Build

```bash
cargo install tauri-cli --version "^2" --locked
cd src-tauri && cargo tauri build --features vulkan   # Windows
cd src-tauri && cargo tauri build --features metal    # macOS
```

You can also use the Tauri CLI through `npx --yes @tauri-apps/cli@^2 build --features vulkan` (this is what CI uses). Use `--features cuda` for NVIDIA. whisper.cpp and llama.cpp both embed ggml, so llama.cpp is linked as a shared library and shipped inside the installer.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the development environment, tests, and the PR flow.

## Contributing

- [CONTRIBUTING.md](CONTRIBUTING.md) — dev environment, tests, PR flow
- [SECURITY.md](SECURITY.md) — how to report vulnerabilities privately
- [CHANGELOG.md](CHANGELOG.md) — release history
- [RELEASING.md](RELEASING.md) — release process (maintainers)
- [docs/index.html](docs/index.html) — landing page (GitHub Pages)
- Issue templates: [bug report](.github/ISSUE_TEMPLATE/bug_report.yml) / [feature request](.github/ISSUE_TEMPLATE/feature_request.yml)

## License

- **The app code is MIT licensed** ([LICENSE](LICENSE)).
- Use of the app is governed by the [floatsubs Terms of Use](licenses/floatsubs-terms.ja.txt) ([English](licenses/floatsubs-terms.en.txt)). The terms include the [PLaMo Community License](https://plamo.preferredai.jp/info/plamo-community-license) as an appendix, accepted together on first launch. The full texts ship in [`licenses/`](licenses/) and are also available from "About" in the app.
  - Personal and non-commercial use is free.
  - Commercial use requires prior registration. Business use by you or your affiliates with annual revenue over JPY 1 billion requires a separate agreement with Preferred Networks.
  - If you publish or distribute subtitles or other output, state that it is PLaMo output. Exported text/SRT does not include the credit, so state it yourself when publishing. The subtitle overlay shows `Built with PLaMo` by default (can be turned off in settings).
- If you publish subtitles for streams or videos, mind the copyright of the source content.
- Whisper models (MIT) and Silero VAD (MIT) follow their own licenses.
- Third-party licenses for the bundled whisper.cpp / llama.cpp / ggml are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
