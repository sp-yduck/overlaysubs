#!/bin/sh
# Set the app version in one place and refresh Cargo.lock.
#
# The version lives only in [workspace.package] of the root Cargo.toml. Both crates inherit it
# with `version.workspace = true`, and Tauri falls back to the Cargo package version because
# src-tauri/tauri.conf.json has no "version" field. Run the tests and commit afterwards; the
# release workflow's `verify` job rejects a tag that does not match this value.
set -eu

usage() {
  echo "usage: $0 X.Y.Z" >&2
  exit 2
}

[ "$#" -eq 1 ] || usage
version=$1
if ! printf '%s' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$'; then
  echo "invalid version: $version" >&2
  usage
fi

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

# Keep the workspace the only source: a config-level version would silently drift.
if grep -Eq '^[[:space:]]*"version"[[:space:]]*:' src-tauri/tauri.conf.json; then
  echo 'error: remove the "version" field from src-tauri/tauri.conf.json; it must come from Cargo.toml.' >&2
  exit 1
fi

# Replace the version inside [workspace.package] and leave every other line untouched.
tmp=$(mktemp)
awk -v v="$version" '
  /^\[workspace\.package\]/ { section = 1 }
  /^\[/ && !/^\[workspace\.package\]/ { section = 0 }
  section && /^[[:space:]]*version[[:space:]]*=/ { sub(/"[^"]*"/, "\"" v "\"") }
  { print }
' Cargo.toml > "$tmp"
mv "$tmp" Cargo.toml

# Cargo.lock records the workspace members' versions. Update just those two entries instead of
# resolving the whole graph, so a release bump works offline and never moves other pins.
tmp=$(mktemp)
awk -v v="$version" '
  /^\[\[package\]\]/ { name = "" }
  /^name = "/ { name = $0; sub(/^name = "/, "", name); sub(/"$/, "", name) }
  name == "overlaysubs" || name == "overlaysubs-core" {
    if (/^version = "/) {
      sub(/^version = "[^"]*"/, "version = \"" v "\"")
      if (name == "overlaysubs") seen_app = 1; else seen_core = 1
    }
  }
  { print }
  END { if (!seen_app || !seen_core) exit 1 }
' Cargo.lock > "$tmp"
mv "$tmp" Cargo.lock

# Validates that [workspace.package] resolves and both members inherit it (no network needed).
cargo metadata --no-deps --format-version 1 > /dev/null

got=$(sh "$root/scripts/version.sh")
if [ "$got" != "$version" ]; then
  echo "error: failed to set the version (found '$got', expected '$version')" >&2
  exit 1
fi

echo "version: $got"
if git rev-parse --git-dir > /dev/null 2>&1; then
  git diff --stat
fi
