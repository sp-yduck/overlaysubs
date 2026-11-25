//! Guards the whisper.cpp patch in `vendor/whisper-rs-sys`.
//!
//! `whisper-rs-sys` 0.15.0 bundles whisper.cpp v1.8.3, whose DTW median filter aborted the process
//! when a decode window advanced 15 frames or fewer (#93). This workspace vendors the crate to
//! carry upstream's fix (ggml-org/whisper.cpp#4096, first released in v1.9.6). If the vendored
//! sources are ever refreshed without the fix — or the vendor directory is dropped while the
//! `[patch.crates-io]` entry stays — the crash comes back silently, so check for it here.
//!
//! Delete this file together with the vendoring once `whisper-rs-sys` bundles whisper.cpp v1.9.6+.

/// The whole body of `median_filter`, taken from the vendored whisper.cpp.
fn median_filter() -> &'static str {
    let src = include_str!("../../../vendor/whisper-rs-sys/whisper.cpp/src/whisper.cpp");
    let start = src.find("static void median_filter(").expect("median_filter is not in the vendored whisper.cpp");
    let rest = &src[start..];
    let end = rest.find("\n}\n").expect("median_filter has no closing brace");
    &rest[..end]
}

#[test]
fn vendored_whisper_cpp_narrows_the_dtw_median_filter() {
    let f = median_filter();

    // Upstream's fix: shrink the filter to the largest odd width that fits instead of aborting.
    assert!(
        f.contains("if (filter_width >= a->ne[2])"),
        "the vendored whisper.cpp lost the DTW median filter fix (#93); see vendor/README.md"
    );
    assert!(
        f.contains("WHISPER_ASSERT(filter_width < a->ne[2] || filter_width == 1)"),
        "the vendored whisper.cpp lost the DTW median filter fix (#93); see vendor/README.md"
    );

    // The unpatched assertion is what aborted: ne[2] = n_frames/2, so 7 < ne[2] fails for windows
    // of 15 frames (150 ms) or fewer.
    assert!(
        !f.contains("WHISPER_ASSERT(filter_width < a->ne[2]);"),
        "the vendored whisper.cpp has the unpatched DTW assert that aborts the process (#93)"
    );
}
