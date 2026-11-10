#!/usr/bin/env bash
# Measures speed and quality of the real pipeline (VAD -> Whisper -> PLaMo) on recorded clips,
# and compares the result with an earlier run.
#
#   scripts/bench.sh <clip.wav | folder of .wav> [baseline.json]
#
# Clips are 16 kHz mono 16-bit WAV (ffmpeg -i in.mp4 -ar 16000 -ac 1 -c:a pcm_s16le clip.wav).
# Optional, next to clip.wav: clip.txt (what is said) and clip.tr.txt (a reference translation);
# with them the report has character error rates, without them only the change from the baseline.
#
# The report goes to target/bench/<commit>.json. To measure a change:
#   git switch main      && scripts/bench.sh clips/            # -> target/bench/<main>.json
#   git switch my-branch && scripts/bench.sh clips/ target/bench/<main>.json
#
# Env: OVERLAYSUBS_ASR / OVERLAYSUBS_MT (model ids from models.json; default whisper-base and
# plamo-3-translate-0.8b-q4_k_m, about 0.7 GB downloaded on first run), OVERLAYSUBS_GPU=1 with
# BENCH_FEATURES=vulkan|cuda|metal. Timings vary between runs: close other apps, and rerun before
# trusting a difference of a few percent.
set -euo pipefail
[ $# -ge 1 ] || { sed -n '2,18p' "$0"; exit 1; }
root="$(cd "$(dirname "$0")/.." && pwd)"
abs() { (cd "$(dirname "$1")" && echo "$PWD/$(basename "$1")"); }
dirty=""; git -C "$root" diff --quiet HEAD || dirty="-dirty"
mkdir -p "$root/target/bench"
export OVERLAYSUBS_OUT="$root/target/bench/$(git -C "$root" rev-parse --short HEAD)$dirty.json"
export OVERLAYSUBS_WAV="$(abs "$1")"
export OVERLAYSUBS_BASELINE="${2:+$(abs "$2")}"
cd "$root"
# --release: a debug build also compiles whisper.cpp and llama.cpp unoptimized.
cargo test --release -p overlaysubs ${BENCH_FEATURES:+--features "$BENCH_FEATURES"} bench_report -- --ignored --nocapture
echo "report: $OVERLAYSUBS_OUT"
