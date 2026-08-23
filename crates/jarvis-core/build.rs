fn main() {
    // link to Vosk lib
    // this crate is the one that actually depends on vosk, so the search path
    // belongs here: dependent binaries inherit it, and the crate's own test
    // binary links too (it has no build script of its own to fall back on).
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let lib_path = std::path::Path::new(&manifest_dir)
        .join("../../lib/windows/amd64");

    println!("cargo:rustc-link-search=native={}", lib_path.display());
}
