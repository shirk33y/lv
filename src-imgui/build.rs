/// Build script: point linker at real import .lib files for Windows cross-compilation.
///
/// pkg/win64/ contains MSVC import libraries (SDL2.lib, mpv.lib) generated
/// from the runtime DLLs via llvm-dlltool. This script adds that directory
/// to the native library search path when targeting Windows.
fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target != "windows" {
        return;
    }

    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let lib_dir = std::path::Path::new(&manifest).join("pkg/win64");
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
}
