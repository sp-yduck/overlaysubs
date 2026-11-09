# Security Policy

Thanks for your interest in the security of OverlaySubs. This page describes the supported versions and how to report a vulnerability.

## Supported versions

Only the latest release is supported. Fixes go to the latest series and are published as a new release when needed.

| Version | Supported |
|---|---|
| 0.1.x (latest release) | ✅ Supported |
| Earlier | ❌ Not supported |

## Reporting a vulnerability

**Do not report vulnerabilities in a public issue.** Use the private channels below.

1. Open the **Security** tab of the repository.
2. Click **Report a vulnerability** (the private report form in GitHub Security Advisories).
   - Direct link: <https://github.com/sp-yduck/overlaysubs/security/advisories/new>

Include the following where possible:

- Affected version and OS (Windows 10/11 x64, macOS 13+ Apple Silicon, and so on)
- Type and impact of the issue (e.g. local file read/write, bypassing model verification, privilege escalation from the UI)
- Reproduction steps (minimal steps are fine)
- Logs, screenshots, or a PoC if needed

**Our target for the first response is within 3 business days.** We decide the fix and the disclosure timing together with the reporter.

## Scope

We consider vulnerabilities in the following areas:

- **The app itself** (the Rust code in `src-tauri/`, the HTML/JS in `ui/`): audio capture, Tauri IPC commands, window settings, permission (capability) configuration in `src-tauri/capabilities/`, and so on.
- **Model downloads and integrity**: URL pinning in `models.json`, SHA-256 verification, resume handling (`.part` files), path handling for download destinations.
- **Local data privacy**: audio, transcripts, and translations are not sent anywhere; there is no telemetry; and the storage location and contents of settings, history, and `terms-accepted`.
- **Distribution artifacts**: the layout of the installers (NSIS / MSI) and the `.dmg`, and bundle configuration.
- **Terms of use and consent flow**: how consent is stored and when it is shown again (versioning of `terms-accepted`).

## Out of scope

The following are generally out of scope. If they affect this app, we still welcome a report.

- Model quality, translation errors, and hallucination. Report them to the upstream projects (whisper.cpp / Silero VAD / PLaMo) or the model providers.
- Vulnerabilities in upstream dependencies (Tauri, cpal, whisper-rs, llama-cpp-2, and so on) themselves. Report them upstream (reports about their impact on this app are welcome).
- **SmartScreen / Gatekeeper warnings.** This is expected behaviour because the distributed builds are unsigned and not notarized, and it is not a security defect. Instructions are in [Warnings on the official site](https://sp-yduck.github.io/overlaysubs/en/#warning).
- Problems with third-party loopback (virtual audio) devices or drivers.
- Phishing, social engineering, and physical access.
- Denial of service (DoS) or attacks that require unrealistic resource consumption.
- Already-public known issues that have been fixed.

## Bounty

**There is no bug bounty.** This project is run by volunteers. We thank reporters with a response and credit (if desired).

## Disclosure

- After publishing a release with the fix, we publish the details in GitHub Security Advisories.
- If you want credit, tell us when you report. Otherwise, we treat you anonymously.
- Please do not publish details before the fix.

## Contact

The GitHub Security Advisories above are the **only** channel. We do not publish the developer's personal email address. For general questions that are not security-related, check the [FAQ on the official site](https://sp-yduck.github.io/overlaysubs/en/#faq) and then open an issue.
