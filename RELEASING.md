# Release Procedure

OverlaySubs is distributed **only through GitHub Releases**. Pushing a `v*` tag makes GitHub Actions
([`.github/workflows/release.yml`](.github/workflows/release.yml)) build the Windows / macOS
installers and attach them to the Release as fixed-name assets.

This document is for repository maintainers. User-facing documentation lives on the official site
(English: [docs/index.html](docs/index.html) / <https://sp-yduck.github.io/overlaysubs/>; Japanese:
[docs/ja/index.html](docs/ja/index.html)). The README is for developers.

---

## 1. How it works

Pushing a tag such as `v0.1.0` runs the following three jobs in order.

| Job | Trigger | What it does |
|---|---|---|
| `verify` | Tag push only | Verifies that the `vX.Y.Z` tag name matches `[workspace.package] version` in the root `Cargo.toml`. Also checks that both crates inherit the version via `version.workspace = true` and that `src-tauri/tauri.conf.json` has no `version`. Also verifies that `productName` is `OverlaySubs`. |
| `build` | Tag push, manual run, PRs labeled `ci:build` | Builds on `windows-latest` (Vulkan) and `macos-15` (Metal, ad-hoc signing), then **renames the artifacts to the fixed names before** uploading them. |
| `publish` | Tag push only | Generates `SHA256SUMS.txt` and publishes the Release as latest. The body contains the download table, workarounds for the warnings, and `## Changes` (generated automatically from PR titles since the previous release). |

Fixed-name assets attached to the Release (the contract referenced by the README and the landing page):

- `overlaysubs-windows-x64-setup.exe` — NSIS installer (the Windows default; no administrator privileges required)
- `overlaysubs-windows-x64.msi` — MSI (for managed deployments; per-machine installation)
- `overlaysubs-macos-arm64.dmg` — for macOS on Apple Silicon
- `SHA256SUMS.txt` — SHA-256 hashes of the three files above

The names are fixed so that the **version-less URL**
`https://github.com/sp-yduck/overlaysubs/releases/latest/download/<name>` can keep being used in the
README, the landing page, and the Release body.
If you rename an asset, update README.md / README.ja.md / docs/index.html / docs/ja/index.html / the
Release body at the same time.

`workflow_dispatch` is a **dry run of the build only**. `verify` and `publish` are skipped and no
Release is created.

**You can do the same dry run from a PR.** Adding the `ci:build` label to a PR runs only the two
`build` jobs and leaves the installers as artifacts of that run. **It works for PRs from forks
too**, so you can install and try a build on real hardware before merging. See §6, "Build
verification for PRs", for the procedure and caveats.

---

## 2. Publishing for the first time (once only)

The repository is currently **private**. While it stays private, people who are not logged in cannot
download the Release files, so it must be made public. `gh` is assumed to be logged in with the
admin scopes (`repo`, `workflow`).

> **Important (Actions billing)**: Actions for private repositories consume minutes, and if billing
> is not set up or the spending limit is 0, jobs do not start and fail within seconds (annotation:
> `The job was not started because recent account payments have failed or your spending limit
> needs to be increased`). **Actions for public repositories are free and unlimited**, so for the
> first release the reliable order is "make public → configure → dry run → tag".

Recommended order:

1. 2.1 Pre-publication checks
2. 2.4 Switch to public
3. 2.5–2.7 Repository settings (description, topics, Pages, vulnerability reporting, labels)
4. 2.2 Build dry run (free once public)
5. 2.8 Release with tag `v0.1.0`
6. 2.9 Post-publication checks (downloads while logged out, Pages)

### 2.1 Pre-publication checks

```bash
# main is up to date and CI is green
gh run list --workflow=ci.yml --limit=3

# No secrets or huge files in history (visual check)
git log --all --diff-filter=A --name-only --pretty=format: | sort -u | grep -Ei '\.env|secret|credential|\.pem|\.p12|password' || echo "clean"
git rev-list --objects --all | git cat-file --batch-check='%(objecttype) %(objectsize) %(rest)' \
  | awk '$1=="blob" && $2>1000000 {print $2, $3}' | sort -rn | head
```

### 2.2 Build dry run (safer after going public)

Before tagging, check that the installers actually build. No Release is created. While the
repository is private, Actions billing must be configured, so it is more reliable to run this
**after making the repository public in 2.4** (Actions are free for public repositories).

```bash
gh workflow run release.yml --ref main
gh run watch "$(gh run list --workflow=release.yml --limit=1 --json databaseId --jq '.[0].databaseId')"

# Check the generated file names and sizes (are they the fixed names?)
RUN_ID=$(gh run list --workflow=release.yml --limit=1 --json databaseId --jq '.[0].databaseId')
# gh run download extracts into directories named after the artifact
gh run download "$RUN_ID" -n overlaysubs-windows-dist
gh run download "$RUN_ID" -n overlaysubs-macos-dist
ls -lh overlaysubs-windows-dist overlaysubs-macos-dist
```

`verify` and `publish` are skipped, the two `build` jobs succeed, and the three files above are all
present — then all is well (artifacts at this point expire after 7 days).

### 2.3 Rename the repository to overlaysubs (done)

This repository has already been renamed to **overlaysubs** (GitHub redirects URLs from the old name,
`floatsubs`). If your local clone's remote still uses the old name, update it with:

```bash
git remote set-url origin https://github.com/sp-yduck/overlaysubs.git
git remote -v
```

Only if you recreated the repository under a different name, run `gh repo rename overlaysubs`
(while it is private).

### 2.4 Switch to public (mind the order)

Download links, badges, and Pages can **return 404 for a period right after going public**. To avoid
that, run "2.8 Initial release" before this section (2.4) while the repository is still private, and
switch to public once the Release and its 4 assets exist (this consumes private-repository Actions
minutes, and macOS is billed at 10× the rate). If you go public first, immediately tag and create
the Release afterward.

```bash
gh repo edit sp-yduck/overlaysubs \
  --visibility public \
  --accept-visibility-change-consequences
```

> **Note**: Making the repository public **exposes the entire commit history**. You can switch it
> back to private, but content that has been made public cannot be taken back. Complete the checks
> in 2.1 first.
>
> Going public also makes **Actions free and unlimited** (no billing configuration is needed as it
> is for private repositories).

### 2.5 Configure the repository description, topics, and Pages

```bash
gh repo edit sp-yduck/overlaysubs \
  --description "Offline desktop app that transcribes and translates PC audio in real time and shows it as a subtitle overlay (Tauri / whisper.cpp / PLaMo 3 Translate)" \
  --homepage "https://sp-yduck.github.io/overlaysubs/" \
  --add-topic tauri --add-topic rust --add-topic whisper-cpp --add-topic llama-cpp \
  --add-topic plamo --add-topic speech-recognition --add-topic realtime-translation \
  --add-topic subtitles --add-topic overlay --add-topic desktop-app \
  --add-topic windows --add-topic macos --add-topic japanese

# GitHub Pages (serve /docs from the main branch as-is)
gh api -X POST repos/sp-yduck/overlaysubs/pages \
  --input - <<'JSON'
{"build_type":"legacy","source":{"branch":"main","path":"/docs"}}
JSON

# Check that it took effect (may take tens of seconds to a few minutes)
curl -sIL -o /dev/null -w '%{http_code}\n' https://sp-yduck.github.io/overlaysubs/
```

To configure it from the GitHub UI: **Settings → Pages → Build and deployment → Source =
"Deploy from a branch", Branch = `main`, folder = `/docs`**. Because `docs/.nojekyll` is present,
no Jekyll processing occurs. The English pages are served from `docs/index.html`, the Japanese pages
from `docs/ja/index.html`, and `docs/en/index.html` is a redirect stub for old `/en/` links.

### 2.6 Enable private vulnerability reporting

[SECURITY.md](SECURITY.md) assumes GitHub's Private vulnerability reporting.

```bash
gh api -X PUT repos/sp-yduck/overlaysubs/private-vulnerability-reporting
```

In the UI, enable **Settings → Code security → Private vulnerability reporting**.

### 2.7 Create labels for dependency update PRs (optional)

`.github/dependabot.yml` applies the `dependencies` / `rust` / `github-actions` labels.
If they do not exist, no labels are applied, but Dependabot itself still works.

```bash
gh label create dependencies    -R sp-yduck/overlaysubs --color 0366d6 --description "Dependency updates"
gh label create rust            -R sp-yduck/overlaysubs --color dea584 --description "Rust / Cargo"
gh label create github-actions  -R sp-yduck/overlaysubs --color 2088ff --description "GitHub Actions"
```

### 2.8 Initial release (v0.1.0)

```bash
git switch main && git pull
git tag -a v0.1.0 -m "OverlaySubs v0.1.0"
git push origin v0.1.0

# Check progress (the Windows / macOS builds take about 30–60 minutes)
gh run watch "$(gh run list --workflow=release.yml --limit=1 --json databaseId --jq '.[0].databaseId')"
```

### 2.9 Post-publication checks

```bash
gh release view v0.1.0

# Check that even people who are not logged in to GitHub can download
# (curl sends no auth headers, so this doubles as the logged-out check)
for f in overlaysubs-windows-x64-setup.exe overlaysubs-windows-x64.msi \
         overlaysubs-macos-arm64.dmg SHA256SUMS.txt; do
  printf '%s: ' "$f"
  curl -sIL -o /dev/null -w '%{http_code}\n' \
    "https://github.com/sp-yduck/overlaysubs/releases/latest/download/$f"
done

# Landing page
curl -sIL -o /dev/null -w '%{http_code}\n' https://sp-yduck.github.io/overlaysubs/
```

When all 4 assets return **200** and Pages also returns **200**, publication is complete. The
README badges (CI / latest release) appear from this point on as well.

---

## 3. Releases from the second one onward

The version lives in exactly one place: `[workspace.package] version` in the root `Cargo.toml`. Both
crates inherit it via `version.workspace = true`, and `src-tauri/tauri.conf.json` has no `version`,
so Tauri uses the Cargo version.

1. **Bump the version in one place** (the script also updates `Cargo.lock`)

   ```bash
   scripts/set-version.sh X.Y.Z
   git diff --stat
   ```

2. Test and verify the build

   ```bash
   cargo test -p overlaysubs-core
   gh workflow run release.yml --ref <branch-name>   # dry run (no Release is created)
   ```

3. Create a PR and merge it into `main`, then push the tag

   ```bash
   git switch main && git pull
   git tag -a vX.Y.Z -m "OverlaySubs vX.Y.Z"
   git push origin vX.Y.Z
   ```

> There is no `CHANGELOG.md`. The `## Changes` section of the Release body is generated automatically
> from the titles of PRs merged since the previous release (excluding `chore(release)`). Please write
> PR titles in English so they reach users.

---

## 4. Troubleshooting

### Build failed

Check the Actions logs. Common causes:

- Windows: Vulkan SDK download failure → rerun the workflow
- Windows: `llama.dll` staging failure → `src-tauri/libs/` and `C:\t\release\build\llama-cpp-sys-2-*\out\bin\`
- macOS: `codesign` verification failure → check that the ad-hoc signing configuration (`--config '{"bundle":{"macOS":{"signingIdentity":"-","hardenedRuntime":false}}}'`) has not disappeared. With ad-hoc signing, hardened runtime must be disabled; otherwise the bundled llama/ggml dylibs are rejected by library validation and the app crashes on launch
- Build timeout (240 minutes) → usually tens of minutes when the cache is warm

You can rerun only the failed jobs.

```bash
gh run rerun <run-id> --failed
```

### A released build crashes with "illegal instruction" on some PCs (0xc000001d)

If release builds get compiled for the GitHub runner's CPU, CPUs without those instructions (such
as AVX-512) terminate immediately with `STATUS_ILLEGAL_INSTRUCTION (0xc000001d)`. If Event Viewer's
"Application Error" shows **exception code `0xc000001d` and the faulting module is the app itself**,
this is it. In whisper.cpp / ggml, `GGML_NATIVE` is ON by default, so unless it is explicitly turned
off, builds are optimized for the build machine's CPU (llama.cpp defaults to OFF; whisper.cpp
receives `GGML_NATIVE=OFF` in the build job of `release.yml`).

When you change `GGML_*` / `CMAKE_*` settings, also update the `key` suffix of rust-cache (currently
`-native-off-no-openssl`). If the cache is restored, C/C++ objects are reused and the configuration
changes do not take effect.

### macOS build crashes right after launch (`Library not loaded: /opt/homebrew/...`)

If the distributed macOS build shows "OverlaySubs cannot be opened because a problem occurred" and
quits on a Mac without Homebrew installed, the bundled llama dylib references the build machine's
(GitHub runner's) Homebrew OpenSSL by absolute path. `LLAMA_OPENSSL` in llama.cpp is ON by default,
and `vendor/cpp-httplib` links the OpenSSL found by `find_package(OpenSSL)` into `libllama-common`
(occurred in v0.2.4: issue #35), so on a Mac without Homebrew, `libssl.4.dylib` cannot be loaded and
dyld exits with `Library missing`.

The `build` job in `release.yml` passes `CMAKE_DISABLE_FIND_PACKAGE_OpenSSL=ON` to stop detection
altogether, and the `Verify macOS bundle is signed` step inspects the dependencies of **every
Mach-O in the bundle** and fails on any absolute path other than a macOS system library or any
`@rpath` that is not bundled. Also update the rust-cache `key` suffix when you change this setting.

The system audio capture (ScreenCaptureKit) binding statically links a small Swift bridge at build
time, so at runtime it references the macOS-provided Swift runtime (`/usr/lib/swift`) via `@rpath`.
The rpath is added to the app binary by `src-tauri/build.rs` (because `rustc-link-arg` emitted by a
dependency's build script does not propagate to the app binary's link), and the check allows
`@rpath/libswift*` only when the Mach-O has `LC_RPATH /usr/lib/swift`. macOS 13 and later provide the
Swift runtime, so bundling it is unnecessary. If the app crashes on launch with
`Library not loaded: @rpath/libswift_Concurrency.dylib`, check that the macOS `rustc-link-arg` in
`src-tauri/build.rs` has not been removed.

### macOS build fails with `cannot find 'MTLTensorDataType' in scope`

The apple-metal Swift bridge that screencapturekit depends on references Metal APIs added in the
macOS 26 SDK (`MTLTensor*`, `MTLSamplerReductionMode`, etc.), so it cannot build with the default
Xcode 16.4 (macOS 15.5 SDK). The macOS build in `release.yml` selects the Xcode 26 preinstalled on
the `macos-15` runner via `xcode-select` before building. If you build locally, also run
`sudo xcode-select -s /Applications/Xcode_26.x.app` (macOS 13 support for the distributed build is
unchanged because tauri-cli sets `MACOSX_DEPLOYMENT_TARGET=13.0`).

### Rebuild the same version

Once the Release has all 4 assets, `verify` considers it "already published" and **skips the build**
(to prevent double publishing and tag mix-ups). To rebuild, delete the Release and the tag, then tag
again.

```bash
gh release delete vX.Y.Z --yes
git push origin :refs/tags/vX.Y.Z
git tag -d vX.Y.Z
git tag -a vX.Y.Z -m "OverlaySubs vX.Y.Z"
git push origin vX.Y.Z
```

### CI / release jobs fail within seconds

If a job fails with `steps=0` and the annotation below appears, the cause is the Actions billing
configuration.

> The job was not started because recent account payments have failed or your spending limit
> needs to be increased.

Fix: **make the repository public** (Actions are free for public repositories), or configure a
payment method and spending limit under GitHub's **Settings → Billing & plans**. If you stay
private, builds will not start after tagging without this configuration either.

### Nothing happens after pushing a tag

- Is the tag in `v0.1.0` format? (`0.1.0` and `release-0.1.0` are not matched)
- Does `release.yml` exist on `main`? (`workflow_dispatch` cannot be run manually unless the file
  exists on the default branch)
- Has the commit been merged into `main`?

### Change asset names or URLs

The fixed names are a contract among README.md / README.ja.md / docs/index.html / docs/ja/index.html /
the Release body / this document. Update the staging names and minimum sizes in `release.yml` and the
expected `release_exists` values in `verify` at the same time, and make all user-facing links
consistent before tagging.

---

## 5. Adding code signing

Current distributed builds are **unsigned on Windows / ad-hoc signed only on macOS (not notarized)**,
so SmartScreen and Gatekeeper warnings appear ([the official site's warnings
section](https://sp-yduck.github.io/overlaysubs/#warning)). To eliminate the warnings, set the
following secrets; the workflow then switches to signing without any changes.

### macOS (requires Apple Developer Program)

| Secret | Description |
|---|---|
| `APPLE_CERTIFICATE` | base64-encoded Developer ID Application certificate (.p12) |
| `APPLE_CERTIFICATE_PASSWORD` | password for the .p12 |
| `APPLE_SIGNING_IDENTITY` | certificate identity name (when this is set, ad-hoc signing is not used) |
| `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID` | when notarizing |

### Windows

Prepare a code signing certificate (OV / EV) and set `bundle.windows.certificateThumbprint` etc. in
`src-tauri/tauri.conf.json`.

Until signing is added, point users to the warnings section on the official site
(<https://sp-yduck.github.io/overlaysubs/#warning>).

---

## 6. Build verification for PRs (including PRs from forks)

Adding the **`ci:build` label** to a PR runs only the `build` jobs in `release.yml` (`verify` /
`publish` are skipped). Because the Windows / macOS installers are actually built and can be
installed before merging, this is useful for verifying changes that touch Tauri bundling or native
dependencies (whisper.cpp / llama.cpp). **It works the same way for PRs from forks.**

### 6.1 One-time only: create the label

```bash
gh label create "ci:build" -R sp-yduck/overlaysubs \
  --color 1d76db --description "Run the Windows / macOS release build for this PR"
```

### 6.2 Run it

```bash
# Add the label to the PR (a maintainer action; fork PRs are fine too)
gh pr edit <PR-number> --add-label "ci:build"
```

- The first build starts as soon as the label is added. After that, every push to the PR rebuilds
  automatically (`synchronize`)
- To rerun on the same PR, remove the label and add it again (`labeled` fires again)
- **PRs from first-time contributors require approval.** Click "Approve and run workflows" on the
  run page, or follow the "Fork pull request workflows from outside collaborators" setting under
  Settings → Actions → General
- Pushing a new commit to the same PR **automatically cancels the previous run** (runs for tag
  pushes are not cancelled)
- For PRs opened before this workflow change is merged into `main`, builds do not run even with the
  label (because `pull_request` runs use the workflow definition from the PR's merge commit)

### 6.3 Fetch the artifacts and try them

```bash
# Find the PR's run (filtering by --event is reliable even for fork branches)
RUN_ID=$(gh run list --workflow=release.yml --event=pull_request \
  --limit=1 --json databaseId --jq '.[0].databaseId')
gh run view "$RUN_ID"

gh run download "$RUN_ID" -n overlaysubs-windows-dist
gh run download "$RUN_ID" -n overlaysubs-macos-dist
ls -lh overlaysubs-windows-dist overlaysubs-macos-dist
```

If the three fixed-name assets (`overlaysubs-windows-x64-setup.exe` /
`overlaysubs-windows-x64.msi` / `overlaysubs-macos-arm64.dmg`) are all present, all is well. PR
artifacts expire after **3 days** (dry runs after 7 days).

### 6.4 Limitations and caveats

- **Secrets are not passed through.** `GITHUB_TOKEN` is also pinned to read-only, so `publish`
  cannot run even if the workflow is rewritten. macOS builds are ad-hoc signed (not notarized), so
  Gatekeeper warnings appear the same way as with distributed builds, but Developer ID signing and
  notarization themselves cannot be verified. For changes you want to verify with signing, have a
  maintainer bring them into a temporary branch and dry-run with
  `gh workflow run release.yml --ref <branch>`
- **`publish` never runs.** Only on tag pushes. PR artifacts are for testing only and are not
  attached to a Release
- Because a PR can rewrite `release.yml` itself, **a green PR build is not evidence for a release**.
  The authority is the build produced by a tag push on `main`
- PR builds do not save caches (so they do not evict the release caches). That makes the first run
  slower
- The timeout is up to 4 hours per OS, for 2 OSes. Actions minutes are free for public
  repositories, but operate so that builds run only when needed via the label
