/// Build script: generate stub .lib files for Windows cross-compilation.
///
/// When cross-compiling to x86_64-pc-windows-msvc from Linux, the system
/// SDL2 and mpv dev packages only provide Linux .so files. The linker needs
/// .lib import libraries. We generate minimal COFF archives so linking
/// succeeds; actual DLLs are provided at runtime on Windows.
fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target != "windows" {
        return;
    }

    let out = std::env::var("OUT_DIR").unwrap();
    let out_path = std::path::Path::new(&out);

    for lib_name in &["SDL2", "mpv"] {
        let lib_file = out_path.join(format!("{}.lib", lib_name));
        if !lib_file.exists() {
            // Minimal COFF archive: just the signature + empty long-names member.
            // This is enough for lld-link to accept it as a valid (empty) import lib.
            let mut ar: Vec<u8> = Vec::new();
            ar.extend_from_slice(b"!<arch>\n"); // archive signature
                                                // First linker member (empty) — 60-byte header + 4-byte body (symbol count = 0)
            ar.extend_from_slice(b"/               0           0     0     0       4         `\n");
            ar.extend_from_slice(&[0u8; 4]); // number of symbols = 0
            std::fs::write(&lib_file, &ar).unwrap();
        }
    }

    println!("cargo:rustc-link-search=native={}", out);
}
