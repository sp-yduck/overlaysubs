#!/usr/bin/env bash
# gen-third-party-notices.sh — regenerate the repository's THIRD_PARTY_NOTICES.md
#
# Regenerate the notices file with:
#
#     ./scripts/gen-third-party-notices.sh
#
# The script resolves the dependency graph with `cargo metadata`, then reads the
# license files that ship inside the local cargo registry and git checkouts
# (cargo honours CARGO_HOME). Run it from anywhere; it writes
# <repo root>/THIRD_PARTY_NOTICES.md.
#
# Requirements: bash, cargo, jq, awk, sed, sort, find. `git` is used to read the
# pinned llama.cpp submodule commit; `curl` is used only when a license text is
# not shipped by any crate in the local cargo cache (currently
# LGPL-2.1-or-later and MIT-0). To regenerate offline, put those texts at
# $THIRD_PARTY_NOTICES_LICENSE_DIR/<SPDX-ID>.txt; otherwise they are fetched
# from the pinned canonical URLs recorded in the generated file.
#
# The output is deterministic: it depends only on Cargo.lock, the license files
# in the cargo cache (including the whisper.cpp / llama.cpp checkouts),
# models.json and the fallback license texts. No timestamps, tool versions or
# absolute host paths are written.
set -euo pipefail

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
export LC_ALL=C

need() {
  command -v "$1" >/dev/null 2>&1 || {
    printf 'error: %s is required to generate THIRD_PARTY_NOTICES.md\n' "$1" >&2
    exit 1
  }
}
for tool in cargo jq awk sed sort find mktemp; do need "$tool"; done

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

out="$root/THIRD_PARTY_NOTICES.md"
models_json="$root/models.json"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

# Print a file with trailing whitespace stripped, leading/trailing blank lines
# removed, every line newline-terminated. Deterministic and markdown-safe.
clean_text() {
  awk 'NF && !first {first = NR} NF {last = NR} {line[NR] = $0}
       END {if (first) for (i = first; i <= last; i++) print line[i]}' "$1"
}

# Print the first copyright line of a license file ("-" when none).
copyright_line() {
  local line
  line=$(grep -m1 -E '^[[:space:]]*Copyright' "$1" 2>/dev/null || true)
  line=${line#"${line%%[![:space:]]*}"}
  if [ -n "$line" ]; then printf '%s' "$line"; else printf '%s' '-'; fi
}

# Fetch a text from a pinned URL into $tmp; used only when no local source has
# the license text. Fails loudly instead of silently emitting an empty section.
fetch_text() {
  local url=$1 dest=$2
  need curl
  printf 'warning: downloading license text from %s\n' "$url" >&2
  curl -fsSL --max-time 120 "$url" -o "$dest" || {
    printf 'error: could not download %s; set THIRD_PARTY_NOTICES_LICENSE_DIR and put the text at <SPDX-ID>.txt\n' "$url" >&2
    exit 1
  }
}

# Indent the crate list entries; one "name version" per line so that a name
# and its version can never be split across lines.
wrap_crates() {
  sed -e 's/^/  /'
}

# ---------------------------------------------------------------------------
# 1. Dependency graph
# ---------------------------------------------------------------------------
printf '==> cargo metadata\n' >&2
cargo metadata --format-version 1 > "$tmp/meta.json"

# name/version -> manifest dir, for every package in the graph.
jq -r '
  .packages[]
  | [.name, .version, (.license // ""), (.repository // .homepage // "-"),
     (.manifest_path | rtrimstr("/Cargo.toml"))]
  | @tsv
' "$tmp/meta.json" > "$tmp/all-packages.tsv"

# Third-party packages only: drop the workspace member ids.
jq -r --argjson members "$(jq -c '.workspace_members' "$tmp/meta.json")" '
  [.packages[]
   | select((.id as $id | $members | index($id)) | not)
   | [.name, .version, (.license // ""), (.repository // .homepage // "-"),
      (.manifest_path | rtrimstr("/Cargo.toml"))]]
  | sort_by(.[0], .[1])
  | .[]
  | @tsv
' "$tmp/meta.json" > "$tmp/crates.tsv"

third_party_count=$(wc -l < "$tmp/crates.tsv" | tr -d ' ')
no_license_count=$(awk -F'\t' '$3 == "" {n++} END {print n + 0}' "$tmp/crates.tsv")

# Split every SPDX expression into the licenses that actually apply. Non-SPDX
# separators used by older crates ("MIT/Apache-2.0", "Apache-2.0 / MIT") are
# treated as OR. "X WITH Y" is kept together as one license unit.
awk -F'\t' '
  {
    expr = $3
    gsub(/[()]/, " ", expr)
    gsub(/\//, " OR ", expr)
    n = split(expr, part, /[[:space:]]+/)
    for (i = 1; i <= n; i++) {
      if (part[i] == "" || part[i] == "OR" || part[i] == "AND") continue
      if (i + 2 <= n && part[i + 1] == "WITH") {
        print part[i] " WITH " part[i + 2] "\t" $1 "\t" $2
        i += 2
      } else {
        print part[i] "\t" $1 "\t" $2
      }
    }
  }
' "$tmp/crates.tsv" > "$tmp/terms.tsv"

# Distinct licenses, most common first, ties alphabetical.
cut -f1 "$tmp/terms.tsv" | sort | uniq -c \
  | awk '{$1 = $1 + 0; print}' \
  | sort -k1,1nr -k2,2 \
  | awk '{$1 = $1 + 0; count = $1; $1 = ""; sub(/^[[:space:]]+/, ""); print count "\t" $0}' \
  > "$tmp/terms-sorted.tsv"
license_count=$(wc -l < "$tmp/terms-sorted.tsv" | tr -d ' ')

# ---------------------------------------------------------------------------
# 2. Bundled native libraries (whisper.cpp and llama.cpp/ggml)
# ---------------------------------------------------------------------------
manifest_of() {
  jq -r --arg n "$1" '
    [.packages[] | select(.name == $n)]
    | sort_by(.version)
    | if length == 0 then "" else .[-1].manifest_path end
  ' "$tmp/meta.json"
}

whisper_sys_manifest=$(manifest_of whisper-rs-sys)
llama_sys_manifest=$(manifest_of llama-cpp-sys-2)
[ -n "$whisper_sys_manifest" ] || { echo 'error: whisper-rs-sys not in dependency graph' >&2; exit 1; }
[ -n "$llama_sys_manifest" ] || { echo 'error: llama-cpp-sys-2 not in dependency graph' >&2; exit 1; }

whisper_sys_version=$(jq -r --arg n whisper-rs-sys '
  [.packages[] | select(.name == $n)] | sort_by(.version) | .[-1].version' "$tmp/meta.json")
llama_sys_version=$(jq -r --arg n llama-cpp-sys-2 '
  [.packages[] | select(.name == $n)] | sort_by(.version) | .[-1].version' "$tmp/meta.json")
llama_cpp_version_crate=$(jq -r --arg n llama-cpp-2 '
  [.packages[] | select(.name == $n)] | sort_by(.version) | .[-1].version' "$tmp/meta.json")
llama_rs_rev=$(jq -r --arg n llama-cpp-sys-2 '
  [.packages[] | select(.name == $n)] | sort_by(.version) | .[-1].source // ""' "$tmp/meta.json" \
  | sed -n 's/.*#\([0-9a-f]\{40\}\)$/\1/p')
llama_rs_rev=${llama_rs_rev:-0364f5da1c6a6ee37f507031f116ce4a09b28594}

whisper_cpp_dir="$(dirname "$whisper_sys_manifest")/whisper.cpp"
whisper_cpp_version=$(sed -n 's/^project("whisper\.cpp" VERSION \([0-9][0-9.]*\)).*/\1/p' \
  "$whisper_cpp_dir/CMakeLists.txt" 2>/dev/null | head -n 1 || true)
whisper_cpp_version=${whisper_cpp_version:-1.8.3}

llama_cpp_dir="$(dirname "$llama_sys_manifest")/llama.cpp"
llama_cpp_version=""
if [ -f "$llama_cpp_dir/CMakeLists.txt" ]; then
  llama_major=$(sed -n 's/^set(LLAMA_VERSION_MAJOR \([0-9][0-9]*\)).*/\1/p' "$llama_cpp_dir/CMakeLists.txt" | head -n 1)
  llama_minor=$(sed -n 's/^set(LLAMA_VERSION_MINOR \([0-9][0-9]*\)).*/\1/p' "$llama_cpp_dir/CMakeLists.txt" | head -n 1)
  llama_patch=$(sed -n 's/^set(LLAMA_VERSION_PATCH \([0-9][0-9]*\)).*/\1/p' "$llama_cpp_dir/CMakeLists.txt" | head -n 1)
  if [ -n "$llama_major" ] && [ -n "$llama_minor" ] && [ -n "$llama_patch" ]; then
    llama_cpp_version="$llama_major.$llama_minor.$llama_patch"
  fi
fi
llama_cpp_version=${llama_cpp_version:-0.8.0}
llama_cpp_commit=$(git -C "$llama_cpp_dir" rev-parse HEAD 2>/dev/null || true)
llama_cpp_commit=${llama_cpp_commit:-8642541b255bfb5dc39cd351a178be09013a55d6}

whisper_license_src="$whisper_cpp_dir/LICENSE"
llama_license_src="$llama_cpp_dir/LICENSE"
if [ -f "$whisper_license_src" ]; then
  clean_text "$whisper_license_src" > "$tmp/whisper-license.txt"
else
  fetch_text "https://raw.githubusercontent.com/ggml-org/whisper.cpp/v${whisper_cpp_version}/LICENSE" "$tmp/whisper-license.txt"
  clean_text "$tmp/whisper-license.txt" > "$tmp/whisper-license-clean.txt"
  mv "$tmp/whisper-license-clean.txt" "$tmp/whisper-license.txt"
fi
if [ -f "$llama_license_src" ]; then
  clean_text "$llama_license_src" > "$tmp/llama-license.txt"
else
  fetch_text "https://raw.githubusercontent.com/ggml-org/llama.cpp/${llama_cpp_commit}/LICENSE" "$tmp/llama-license.txt"
  clean_text "$tmp/llama-license.txt" > "$tmp/llama-license-clean.txt"
  mv "$tmp/llama-license-clean.txt" "$tmp/llama-license.txt"
fi
whisper_copyright=$(copyright_line "$tmp/whisper-license.txt")
llama_copyright=$(copyright_line "$tmp/llama-license.txt")

# ---------------------------------------------------------------------------
# 3. License texts for the Rust dependencies
# ---------------------------------------------------------------------------
# The text for a license unit is taken from the first crate (alphabetically)
# that offers that license and ships a matching license file. A license file is
# accepted when its name or its contents clearly identify the license.
term_matches() {
  local term=$1 file=$2 base
  base=$(basename "$file" | tr '[:upper:]' '[:lower:]')
  case "$term" in
    "MIT")
      case "$base" in
        *mit*) grep -qE '^[[:space:]]*Copyright' "$file" && return 0 ; return 1 ;;
      esac
      grep -qF 'Permission is hereby granted, free of charge' "$file" \
        && grep -qE '^[[:space:]]*Copyright' "$file"
      ;;
    "Apache-2.0")
      case "$base" in *apache*) return 0 ;; esac
      grep -qF 'Apache License' "$file" && grep -qF 'Version 2.0, January 2004' "$file"
      ;;
    "Apache-2.0 WITH LLVM-exception")
      case "$base" in *llvm*) return 0 ;; esac
      return 1
      ;;
    "Zlib")
      case "$base" in *zlib*) return 0 ;; esac
      grep -qF "This software is provided 'as-is'" "$file"
      ;;
    "Unicode-3.0")
      case "$base" in *unicode*) return 0 ;; esac
      grep -qiF 'UNICODE LICENSE V3' "$file"
      ;;
    "Unlicense")
      case "$base" in *unlicense*) return 0 ;; esac
      grep -qF 'free and unencumbered software released into the public domain' "$file"
      ;;
    "BSD-3-Clause")
      case "$base" in *bsd-3*|*bsd3*) return 0 ;; esac
      grep -qF 'Redistribution and use in source and binary forms' "$file" \
        && grep -qF 'Neither the name of' "$file"
      ;;
    "BSD-2-Clause")
      case "$base" in *bsd-2*|*bsd2*) return 0 ;; esac
      grep -qF 'Redistribution and use in source and binary forms' "$file" \
        && ! grep -qF 'Neither the name of' "$file" \
        && ! grep -qF 'Apache License' "$file"
      ;;
    "ISC")
      case "$base" in *isc*) return 0 ;; esac
      grep -qF 'Permission to use, copy, modify, and/or distribute this software for any purpose' "$file" \
        && grep -qF 'provided that the above copyright notice' "$file"
      ;;
    "MPL-2.0")
      case "$base" in *mpl*) return 0 ;; esac
      grep -qF 'Mozilla Public License Version 2.0' "$file"
      ;;
    "BSL-1.0")
      case "$base" in *boost*) return 0 ;; esac
      grep -qF 'Boost Software License - Version 1.0' "$file"
      ;;
    "CC0-1.0")
      case "$base" in *cc0*) return 0 ;; esac
      grep -qF 'CC0 1.0 Universal' "$file"
      ;;
    "MIT-0")
      case "$base" in *mit-0*|*mit0*) return 0 ;; esac
      return 1
      ;;
    "0BSD")
      case "$base" in *0bsd*|*0-bsd*) return 0 ;; esac
      return 1
      ;;
    "CDLA-Permissive-2.0")
      grep -qF 'Community Data License Agreement' "$file"
      ;;
    "LGPL-2.1-or-later")
      case "$base" in *lgpl*) return 0 ;; esac
      return 1
      ;;
    *)
      return 1
      ;;
  esac
}

fallback_url() {
  case "$1" in
    "LGPL-2.1-or-later") printf '%s' 'https://www.gnu.org/licenses/old-licenses/lgpl-2.1.txt' ;;
    "MIT-0") printf '%s' 'https://raw.githubusercontent.com/spdx/license-list-data/v3.27.0/text/MIT-0.txt' ;;
    *) printf '' ;;
  esac
}

# Emit one line describing where a license text came from; never a host path.
text_source_note=""

resolve_license_text() {
  local term=$1 source="" crate_name="" crate_version="" file=""
  local cname cver cdir scan_dir
  : > "$tmp/candidates.tsv"
  awk -F'\t' -v t="$term" '$1 == t {print $2 "\t" $3}' "$tmp/terms.tsv" | sort -u > "$tmp/candidates.tsv"
  while IFS=$'\t' read -r cname cver; do
    cdir=$(awk -F'\t' -v n="$cname" -v v="$cver" '$1 == n && $2 == v {print $5; exit}' "$tmp/crates.tsv")
    [ -n "$cdir" ] || continue
    for scan_dir in "$cdir" "$(dirname "$cdir")"; do
      [ -d "$scan_dir" ] || continue
      # Only walk one level up for git checkouts (their LICENSE files can live
      # at the repository root); registry packages ship them inside the crate.
      if [ "$scan_dir" != "$cdir" ]; then
        case "$cdir" in
          */registry/src/*) continue ;;
        esac
      fi
      while IFS= read -r file; do
        if term_matches "$term" "$file"; then
          source="local"
          crate_name=$cname
          crate_version=$cver
          break 3
        fi
      done < <(find "$scan_dir" -maxdepth 1 -type f \
        \( -iname 'LICENSE*' -o -iname 'LICENCE*' -o -iname 'COPYING*' -o -iname 'UNLICENSE*' \) \
        | sort)
    done
  done < "$tmp/candidates.tsv"

  if [ "$source" = "local" ]; then
    clean_text "$file" > "$tmp/license-text.txt"
    text_source_note="the $(basename "$file") file in the $crate_name $crate_version package"
    return 0
  fi

  # Optional offline fallback, then the pinned canonical URL.
  local override=""
  if [ -n "${THIRD_PARTY_NOTICES_LICENSE_DIR:-}" ] && [ -f "$THIRD_PARTY_NOTICES_LICENSE_DIR/$term.txt" ]; then
    override="$THIRD_PARTY_NOTICES_LICENSE_DIR/$term.txt"
  fi
  if [ -n "$override" ]; then
    clean_text "$override" > "$tmp/license-text.txt"
    text_source_note="canonical text supplied via THIRD_PARTY_NOTICES_LICENSE_DIR ($term.txt)"
    return 0
  fi
  local url
  url=$(fallback_url "$term")
  if [ -z "$url" ]; then
    printf 'error: no license text found locally for "%s". Add a fallback in %s.\n' \
      "$term" "$0" >&2
    exit 1
  fi
  fetch_text "$url" "$tmp/license-fetched.txt"
  clean_text "$tmp/license-fetched.txt" > "$tmp/license-text.txt"
  text_source_note="canonical text from $url"
}

# ---------------------------------------------------------------------------
# 4. Compose THIRD_PARTY_NOTICES.md
# ---------------------------------------------------------------------------
printf '==> composing %s\n' "$out" >&2
{
  cat <<'HEADER'
# Third-Party Notices

OverlaySubs is distributed with, and depends on, third-party open-source
software. This file reproduces the license notices and license texts that
apply to the distributed builds.

- **Bundled native libraries.** Every installer produced from this repository
  (Windows `.exe` / `.msi`, macOS `.dmg`) contains compiled native code from
  **whisper.cpp** and ships the **llama.cpp / ggml** shared libraries. Both are
  MIT-licensed; their copyright notices and license texts are reproduced in
  [§1](#1-bundled-native-libraries).
- **Rust dependencies.** The application statically links the third-party Rust
  crates listed in [§3](#3-rust-dependency-inventory). All of them are covered
  by the license texts reproduced in [§4](#4-license-texts).
- **Where this notice is installed.** The installers bundle this file so that it
  is available on end-user machines: Windows places it in the installation
  directory next to `overlaysubs.exe`; macOS places it at
  `OverlaySubs.app/Contents/Resources/THIRD_PARTY_NOTICES.md`.
- **Downloaded at runtime (not part of the installers).** On first run the app
  downloads model files from Hugging Face: OpenAI Whisper GGML models (MIT),
  Silero VAD (MIT) and PLaMo 3 Translate models. PLaMo is **not** open source;
  it is governed by the PLaMo Community License (revised November 9, 2026),
  provided by Preferred Networks, Inc. Personal and academic use are free;
  commercial use requires prior registration with PFN:
  <https://plamo.preferredai.jp/info/plamo-community-license-en>

  The OverlaySubs terms of use are bundled in this repository and shipped with
  the app:
  - [`licenses/overlaysubs-terms.ja.txt`](https://github.com/sp-yduck/overlaysubs/blob/main/licenses/overlaysubs-terms.ja.txt)
  - [`licenses/overlaysubs-terms.en.txt`](https://github.com/sp-yduck/overlaysubs/blob/main/licenses/overlaysubs-terms.en.txt)

  See [§2](#2-runtime-downloaded-model-assets) for the pinned model revisions.

This file is generated by `scripts/gen-third-party-notices.sh`; do not edit it
by hand. Regenerate it with:

```sh
./scripts/gen-third-party-notices.sh
```

HEADER

  # --- §1 bundled native libraries -----------------------------------------
  cat <<'SECTION1'
## 1. Bundled native libraries

The installers bundle the following native libraries. whisper.cpp is linked
statically into the application binary; the llama.cpp / ggml shared libraries
are shipped as separate files (Windows: `libs/*.dll` next to the executable,
macOS: `Contents/Frameworks/` inside the `.app`). The `ggml` code used by both
projects is part of each upstream repository and is covered by the same MIT
license.

SECTION1
  printf '### 1.1 whisper.cpp — MIT\n\n'
  printf 'Version: whisper.cpp %s (vendored in the whisper-rs-sys %s crate)\n\n' \
    "$whisper_cpp_version" "$whisper_sys_version"
  printf 'Upstream: <https://github.com/ggml-org/whisper.cpp>\n\n'
  printf 'Copyright: %s\n\n' "$whisper_copyright"
  printf 'License text:\n\n```text\n'
  cat "$tmp/whisper-license.txt"
  printf '```\n\n'

  printf '### 1.2 llama.cpp / ggml — MIT\n\n'
  printf 'Version: llama.cpp %s, upstream commit %s\n\n' \
    "$llama_cpp_version" "$llama_cpp_commit"
  printf 'Source: git submodule at `llama-cpp-sys-2/llama.cpp` in the llama-cpp-rs\n'
  printf 'repository, pulled by the `llama-cpp-2` dependency at llama-cpp-rs commit %s\n' "$llama_rs_rev"
  printf '(crate versions: llama-cpp-2 %s / llama-cpp-sys-2 %s).\n\n' \
    "$llama_cpp_version_crate" "$llama_sys_version"
  printf 'Upstream: <https://github.com/ggml-org/llama.cpp>\n\n'
  printf 'Copyright: %s\n\n' "$llama_copyright"
  printf 'License text:\n\n```text\n'
  cat "$tmp/llama-license.txt"
  printf '```\n\n'

  # --- §2 runtime model assets ---------------------------------------------
  cat <<'SECTION2'
## 2. Runtime-downloaded model assets

These files are **not** included in the installers. OverlaySubs downloads them
from Hugging Face on first run and verifies each file against the size and
SHA-256 values declared in [`models.json`](https://github.com/sp-yduck/overlaysubs/blob/main/models.json). The MIT license text
that covers the Whisper and Silero VAD assets is reproduced in [§4
("MIT")](#4-license-texts); the PLaMo assets are governed by the PLaMo
Community License (revised November 9, 2026) linked in the header of this
document.

| Model file | Hugging Face repository | Pinned revision | License |
|---|---|---|---|
SECTION2
  jq -r '.models[]
    | (.url | capture("^https://huggingface\\.co/(?<repo>[^/]+/[^/]+)/resolve/(?<rev>[0-9a-f]{40})/(?<file>[^/]+)$"))
    | [ .file, .repo, .rev ] | @tsv' "$models_json" > "$tmp/models.tsv"
  while IFS=$'\t' read -r file repo rev; do
    case "$repo" in
      ggerganov/whisper.cpp) license='MIT (OpenAI Whisper weights, GGML conversion)' ;;
      ggml-org/whisper-vad) license='MIT (Silero VAD)' ;;
      pfnet/plamo-3-translate-*) license='PLaMo Community License, revised 2026-11-09 (not open source; see <https://plamo.preferredai.jp/info/plamo-community-license-en>)' ;;
      *) license='see model card' ;;
    esac
    printf '| %s | `%s` | `%s` | %s |\n' "$file" "$repo" "$rev" "$license"
  done < "$tmp/models.tsv"
  printf '\n'
  printf 'Attribution for the MIT-licensed model weights and the VAD model:\n\n'
  printf -- '- Whisper model weights: Copyright (c) 2022 OpenAI (MIT). The GGML\n'
  printf '  conversions are published in the ggerganov/whisper.cpp repository.\n'
  printf -- '- Silero VAD: Copyright (c) 2020-present Silero Team (MIT). Upstream\n'
  printf '  repository: <https://github.com/snakers4/silero-vad>.\n'
  printf -- '- PLaMo 3 Translate model weights: PLaMo Community License (revised\n'
  printf '  November 9, 2026), provided by Preferred Networks, Inc. Personal and\n'
  printf '  academic use are free; commercial use requires prior registration with\n'
  printf '  PFN. See\n'
  printf '  <https://plamo.preferredai.jp/info/plamo-community-license-en>.\n\n'

  # --- §3 Rust dependency inventory ----------------------------------------
  printf '## 3. Rust dependency inventory\n\n'
  printf 'Generated from the resolved Cargo dependency graph\n'
  printf '(`cargo metadata --format-version 1`, default features, all target\n'
  printf 'platforms). The two workspace crates `overlaysubs` and `overlaysubs-core` are\n'
  printf 'excluded. The license column reproduces each package'"'"'s `license` field\n'
  printf 'verbatim (SPDX expression); the texts are reproduced in [§4](#4-license-texts).\n\n'
  printf -- '- Third-party crates: **%s**\n' "$third_party_count"
  printf -- '- Distinct license units: **%s**\n' "$license_count"
  printf -- '- Crates with no `license` field: **%s**' "$no_license_count"
  if [ "$no_license_count" -eq 0 ]; then
    printf ' (every crate declares a license; none required a "see crate" fallback)\n\n'
  else
    printf ' — these are marked `see crate` in the table below\n\n'
  fi
  printf '| Crate | Version | License | Repository |\n'
  printf '|---|---|---|---|\n'
  awk -F'\t' '{
    license = ($3 == "" ? "see crate" : $3)
    gsub(/\|/, "\\|", license)
    repo = $4
    gsub(/\|/, "\\|", repo)
    print "| `" $1 "` | " $2 " | " license " | " repo " |"
  }' "$tmp/crates.tsv"
  printf '\n'

  # --- §4 license texts -----------------------------------------------------
  printf '## 4. License texts\n\n'
  printf 'Each section below reproduces the full text of one license as shipped in\n'
  printf 'the corresponding crate package, together with the third-party crates (from\n'
  printf '[§3](#3-rust-dependency-inventory)) that are offered under it. Most crates\n'
  printf 'are dual- or multi-licensed, so they appear under more than one license;\n'
  printf 'the combined work may be distributed under any one of the licenses that\n'
  printf 'each crate offers.\n\n'
  printf 'The text was taken from the license file of the first crate (alphabetically)\n'
  printf 'that offers the license and ships a matching file; the source is noted under\n'
  printf 'each section.\n\n'

  section=0
  while IFS=$'\t' read -r count term; do
    section=$((section + 1))
    resolve_license_text "$term"
    if [ "$count" -eq 1 ]; then noun=crate; else noun=crates; fi
    printf '### 4.%s %s (%s %s)\n\n' "$section" "$term" "$count" "$noun"
    case "$term" in
      "LGPL-2.1-or-later")
        printf 'Note: `r-efi` offers LGPL-2.1-or-later only as one of three\n'
        printf 'alternatives (MIT OR Apache-2.0 OR LGPL-2.1-or-later); the distributed\n'
        printf 'build uses the permissive alternatives. The text is reproduced for\n'
        printf 'completeness.\n\n'
        ;;
      "MIT-0")
        printf 'Note: `dunce` offers MIT-0 only as one of three alternatives\n'
        printf '(CC0-1.0 OR MIT-0 OR Apache-2.0). The canonical SPDX text keeps the\n'
        printf 'placeholder (<YEAR> <COPYRIGHT HOLDER>) copyright line.\n\n'
        ;;
    esac
    printf 'Applies to the following crates (name version):\n\n```text\n'
    awk -F'\t' -v t="$term" '$1 == t {print $2 " " $3}' "$tmp/terms.tsv" | sort -u | wrap_crates
    printf '```\n\n'
    printf 'Text source: %s.\n\n' "$text_source_note"
    printf '```text\n'
    cat "$tmp/license-text.txt"
    printf '```\n\n'
  done < "$tmp/terms-sorted.tsv"
} > "$tmp/THIRD_PARTY_NOTICES.md"

mv "$tmp/THIRD_PARTY_NOTICES.md" "$out"
printf '==> wrote %s (%s bytes)\n' "$out" "$(wc -c < "$out" | tr -d ' ')" >&2
