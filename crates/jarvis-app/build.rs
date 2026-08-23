fn main() {
    // link to Vosk lib
    // println!("cargo:rustc-link-lib=libvosk.dll");

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let lib_path = std::path::Path::new(&manifest_dir)
        .join("..\\..\\lib\\windows\\amd64");
    
    println!("cargo:rustc-link-search=native={}", lib_path.display());

    // muda (via tray-icon) calls TaskDialogIndirect, which only comctl32 v6
    // exports. Without this manifest dependency the debug build fails to start
    // with "entry point not found" - in release the call is optimized away, so
    // the missing import only shows up in debug.
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bins=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'");
}
