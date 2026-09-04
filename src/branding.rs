//! Product branding constants for RusticGU.

/// User-facing product name for the main desktop application.
pub const APP_NAME: &str = "RusticGU";

/// Built-in app version (About, update checks).
///
/// Defaults to `Cargo.toml`. Nightly CI sets `RUSTICGU_VERSION` so the binary
/// reports `X.Y.Z-nightly.YYYYMMDDHHMMSS` without rewriting the crate version.
pub const APP_VERSION: &str = env!("RUSTICGU_VERSION");

/// User-facing name for the dedicated self-update helper process.
pub const UPDATER_NAME: &str = "RusticGU Updater";

/// On-disk updater binary name (next to `rusticgu.exe` in the install dir).
pub const UPDATER_EXE_NAME: &str = "rusticgu-updater.exe";

/// About / subtitle line.
pub const APP_TAGLINE: &str = "Game library compact launcher";

/// GitHub repository owner (update feed + release links).
pub const GITHUB_OWNER: &str = "JustNak";

/// GitHub repository name (update feed + release links).
pub const GITHUB_REPO: &str = "RusticGU";

/// NSIS installer asset name published on every GitHub Release.
pub const SETUP_ASSET_NAME: &str = "RusticGU-windows-x64-setup.exe";

/// Windows named-pipe path used by the single-instance activate server.
pub const PIPE_NAME: &str = r"\\.\pipe\rusticgu.v1";

/// AppUserModelID for taskbar / Start Menu identity (must match installer shortcuts).
#[allow(dead_code)]
pub const APP_USER_MODEL_ID: &str = "com.rusticgu.app";

/// AppUserModelID for the updater helper (kept distinct so it does not group as the main app).
#[allow(dead_code)]
pub const UPDATER_USER_MODEL_ID: &str = "com.rusticgu.updater";

/// App data folder under `%APPDATA%` / XDG.
pub const APP_DATA_DIR_NAME: &str = "RusticGU";

/// Relative path to the multi-size Windows icon (from assets root).
pub const APP_ICON_ICO: &str = "brand/icon.ico";

/// Relative path to the square brand mark PNG.
#[allow(dead_code)]
pub const APP_ICON_PNG: &str = "brand/icon-256.png";

/// Dark-theme sidebar / chrome mark (light glyph on dark field).
pub const APP_LOGO_DARK: &str = "brand/logo.png";

/// Light-theme sidebar / chrome mark (dark glyph on light field).
pub const APP_LOGO_LIGHT: &str = "brand/logo-light.png";

/// Vector brand mark (crab fused with a gamepad).
#[allow(dead_code)]
pub const APP_LOGO_SVG: &str = "brand/logo.svg";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branding_constants_match_product() {
        assert_eq!(APP_NAME, "RusticGU");
        assert_eq!(UPDATER_NAME, "RusticGU Updater");
        assert_eq!(UPDATER_EXE_NAME, "rusticgu-updater.exe");
        assert_eq!(GITHUB_OWNER, "JustNak");
        assert_eq!(GITHUB_REPO, "RusticGU");
        assert_eq!(SETUP_ASSET_NAME, "RusticGU-windows-x64-setup.exe");
        assert_eq!(PIPE_NAME, r"\\.\pipe\rusticgu.v1");
        assert_eq!(APP_USER_MODEL_ID, "com.rusticgu.app");
        assert_eq!(UPDATER_USER_MODEL_ID, "com.rusticgu.updater");
        assert_eq!(APP_DATA_DIR_NAME, "RusticGU");
        assert!(APP_TAGLINE.to_ascii_lowercase().contains("game"));
        assert!(!APP_TAGLINE
            .to_ascii_lowercase()
            .contains("download manager"));
        assert!(!APP_VERSION.is_empty());
        assert_eq!(APP_LOGO_SVG, "brand/logo.svg");
        let svg = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join(APP_LOGO_SVG);
        let text = std::fs::read_to_string(&svg).expect("brand svg");
        assert!(text.contains("<svg"), "logo.svg should be an svg");
        assert!(text.contains("#3298a3"), "logo.svg should keep teal");
        assert!(text.contains("#dc953a"), "logo.svg should keep gold");
        assert!(text.contains("#c2206d"), "logo.svg should keep magenta");
    }
}
