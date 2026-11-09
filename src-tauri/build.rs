fn main() {
    // whisper.cpp links ggml statically and llama.cpp ships its own ggml as a shared lib.
    // On ELF, keep the static copy private so libllama binds to its own ggml.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-arg=-Wl,--exclude-libs,ALL");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
        // Cargo applies a dependency's build-script `rustc-link-arg` only to that dependency's
        // own targets, so screencapturekit's Swift-runtime rpath never reaches this binary. Add
        // it here instead: the static Swift bridge records @rpath/libswift_Concurrency.dylib and
        // macOS (13+, our minimum) ships the Swift runtime in /usr/lib/swift. Without this rpath
        // dyld aborts at launch ("Library not loaded: @rpath/libswift_Concurrency.dylib").
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
        // Same propagation rule for ScreenCaptureKit: the bridge is a static library, so the
        // framework has to be named on this link. Weak linking matches the crate's own intent.
        println!("cargo:rustc-link-arg=-Wl,-weak_framework,ScreenCaptureKit");
    }
    tauri_build::build()
}
