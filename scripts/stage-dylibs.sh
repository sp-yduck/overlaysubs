#!/bin/sh
# Collect llama.cpp's shared libs (real files, under every linked name) for bundling, and sherpa-onnx's
# (its C API and ONNX Runtime, for speaker colors), which its build script copies to the profile directory.
set -e
root=$(git rev-parse --show-toplevel)
out="$root/target/frameworks"
rm -rf "$out" && mkdir -p "$out"
for lib in "$root"/target/release/build/llama-cpp-sys-2-*/out/lib/*.dylib; do
  cp -L "$lib" "$out/"
done
for lib in "$root"/target/release/libsherpa-onnx*.dylib "$root"/target/release/libonnxruntime*.dylib; do
  [ -e "$lib" ] && cp -L "$lib" "$out/"
done
ls "$out"
