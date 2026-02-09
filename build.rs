/// Build script:
/// 1. Bake git short hash into the binary as GIT_HASH env var.
/// 2. Point linker at real import .lib files for Windows cross-compilation.
fn main() {
    // ── Git hash ─────────────────────────────────────────────────────
    let hash = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=GIT_HASH={}", hash.trim());

    // Rerun when HEAD changes (new commits)
    println!("cargo:rerun-if-changed=.git/HEAD");
    // Also track the ref file HEAD points to (e.g. refs/heads/main)
    if let Ok(head) = std::fs::read_to_string(".git/HEAD") {
        if let Some(refpath) = head.strip_prefix("ref: ") {
            println!("cargo:rerun-if-changed=.git/{}", refpath.trim());
        }
    }

    // ── Windows link path + icon ─────────────────────────────────────
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target == "windows" {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let lib_dir = std::path::Path::new(&manifest).join("pkg/win64");
        println!("cargo:rustc-link-search=native={}", lib_dir.display());

        let _ = embed_resource::compile("pkg/win64/lv.rc", embed_resource::NONE);
    }
}
