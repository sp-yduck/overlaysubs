#!/bin/sh
# Print the single-source app version, read from [workspace.package] in the root Cargo.toml.
#
# The member crates inherit the version (`version.workspace = true`) and Tauri falls back to
# the Cargo package version, so this is the only place a release bump has to touch.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

awk -F'"' '
  /^\[workspace\.package\]/ { section = 1; next }
  /^\[/ { section = 0 }
  section && /^[[:space:]]*version[[:space:]]*=/ { print $2; exit }
' "$root/Cargo.toml"
