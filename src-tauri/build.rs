fn main() {
    // whisper.cpp links ggml statically and llama.cpp ships its own ggml as a shared lib.
    // On ELF, keep the static copy private so libllama binds to its own ggml.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-arg=-Wl,--exclude-libs,ALL");
    }
    tauri_build::build()
}
