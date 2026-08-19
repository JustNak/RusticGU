//! Embed Windows version resources so Task Manager / Explorer show "RusticGU".
//!
//! UAC / DPI manifests for the main app come from GPUI (`windows-manifest`).
//! The dedicated updater embeds its own asInvoker manifest — see
//! `apps/updater/build.rs` and `assets/windows/app.manifest`.

fn main() {
    // Nightly CI stamps a unique version via RUSTICGU_VERSION without rewriting Cargo.toml
    // (so cargo-packager / Windows ProductVersion stay on the stable x.y.z triple).
    let version = std::env::var("RUSTICGU_VERSION")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into()));
    println!("cargo:rerun-if-env-changed=RUSTICGU_VERSION");
    println!("cargo:rustc-env=RUSTICGU_VERSION={version}");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let mut res = winresource::WindowsResource::new();
    res.set("ProductName", "RusticGU");
    res.set("FileDescription", "RusticGU");
    res.set("CompanyName", "JustNak");
    res.set("LegalCopyright", "Copyright (c) JustNak");
    res.set("InternalName", "rusticgu");
    res.set("OriginalFilename", "rusticgu.exe");
    res.set_icon("assets/brand/icon.ico");

    if let Err(error) = res.compile() {
        // Don't fail cross-tooling environments that lack a resource compiler.
        println!("cargo:warning=winresource failed to embed version info: {error}");
    }
}
