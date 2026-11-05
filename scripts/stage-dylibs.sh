#!/bin/sh
# Collect llama.cpp's shared libs (real files, under every linked name) for bundling.
set -e
root=$(git rev-parse --show-toplevel)
out="$root/target/frameworks"
rm -rf "$out" && mkdir -p "$out"
for lib in "$root"/target/release/build/llama-cpp-sys-2-*/out/lib/*.dylib; do
  cp -L "$lib" "$out/"
done
ls "$out"
