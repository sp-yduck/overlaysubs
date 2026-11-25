# Vendored dependencies

## whisper-rs-sys 0.15.0

`vendor/whisper-rs-sys` is the published `whisper-rs-sys` 0.15.0 crate (from crates.io) with one
patch, applied to `whisper.cpp/src/whisper.cpp`:

```diff
-    WHISPER_ASSERT(filter_width < a->ne[2]);
+    // A window that advanced only a few frames has fewer audio tokens than
+    // the filter is wide: narrow the filter instead of aborting (1: none).
+    if (filter_width >= a->ne[2]) {
+        filter_width = (a->ne[2] % 2) ? (int) a->ne[2] - 2 : (int) a->ne[2] - 1;
+        if (filter_width < 1) {
+            filter_width = 1;
+        }
+    }
+    WHISPER_ASSERT(filter_width < a->ne[2] || filter_width == 1);
```

This is upstream whisper.cpp's fix from [ggml-org/whisper.cpp#4096](https://github.com/ggml-org/whisper.cpp/pull/4096)
(commit `a46fadb`, merged 2026-10-19, first released in v1.9.6), and the function is otherwise
identical to v1.8.3.

Why: with DTW token timestamps on, `median_filter` asserts `filter_width < a->ne[2]` with
`filter_width = 7` and `ne[2] = n_audio_tokens = n_frames/2`. A decode window that advances 15
frames or fewer (a slice of 100-160 ms, or a short last segment) therefore aborts the whole process
inside `whisper_full`, which is the crash in [#93](https://github.com/sp-yduck/overlaysubs/issues/93).
The patch narrows the filter for such windows instead of aborting.

`crates/core/tests/vendored_whisper_cpp.rs` fails if the patch is missing, so a future refresh of the
vendored sources cannot silently drop it.

To remove this vendoring, once a released `whisper-rs-sys` bundles whisper.cpp v1.9.6 or newer:

1. delete `vendor/whisper-rs-sys` and the `[patch.crates-io]` section in the root `Cargo.toml`;
2. delete `crates/core/tests/vendored_whisper_cpp.rs`;
3. run `cargo update -p whisper-rs-sys` and keep the resulting `Cargo.lock`.

Licenses: `whisper-rs-sys` is Unlicense; the bundled whisper.cpp and ggml are MIT (see
`vendor/whisper-rs-sys/whisper.cpp/LICENSE`).
