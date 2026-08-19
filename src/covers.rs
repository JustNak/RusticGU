//! Steam / extra-store cover art: CDN URLs, AppData cache, monogram fallback.
//!
//! Never paints a broken-image card. Missing art becomes a monogram tile.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui::{RenderImage, SharedString};
use image::{Frame, RgbaImage};
use smallvec::SmallVec;

use crate::library::{LibraryStore, LibraryTitle};

/// Primary Steam portrait CDN (library poster).
pub const STEAM_CDN_PRIMARY: &str = "https://cdn.cloudflare.steamstatic.com/steam/apps";
/// Fallback Steam CDN host.
pub const STEAM_CDN_FALLBACK: &str = "https://steamcdn-a.akamaihd.net/steam/apps";

const FETCH_TIMEOUT: Duration = Duration::from_secs(12);
const MIN_IMAGE_BYTES: usize = 64;

/// How a title should be drawn on the gallery wall.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverKind {
    /// Cached or local raster art.
    Art,
    /// Title + store badge tile. Used when no usable image exists.
    Monogram,
}

/// Letters shown on a monogram tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Monogram {
    pub initials: String,
    pub title: String,
    pub store: SharedString,
}

impl Monogram {
    pub fn from_title(title: &LibraryTitle) -> Self {
        Self {
            initials: monogram_initials(&title.name),
            title: title.name.clone(),
            store: SharedString::from(title.store.badge()),
        }
    }
}

/// Steam portrait URL order: library_600x900 on both CDNs, then hero, then header.
pub fn steam_cover_urls(app_id: u32) -> Vec<String> {
    let mut urls = Vec::with_capacity(6);
    for file in ["library_600x900.jpg", "library_hero.jpg", "header.jpg"] {
        urls.push(format!("{STEAM_CDN_PRIMARY}/{app_id}/{file}"));
        urls.push(format!("{STEAM_CDN_FALLBACK}/{app_id}/{file}"));
    }
    urls
}

/// `%APPDATA%/RusticGU/covers/steam/{appid}.jpg` (or the given app-data root).
pub fn steam_cover_cache_path(app_data_root: &Path, app_id: u32) -> PathBuf {
    app_data_root
        .join("covers")
        .join("steam")
        .join(format!("{app_id}.jpg"))
}

pub fn extra_cover_cache_path(app_data_root: &Path, title_id: &str) -> PathBuf {
    let safe: String = title_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    app_data_root
        .join("covers")
        .join("extra")
        .join(format!("{safe}.img"))
}

/// Decide art vs monogram from already-resolved files. No network.
#[allow(dead_code)]
pub fn cover_kind_for(cached: Option<&Path>, local_index: Option<&Path>) -> CoverKind {
    if cached.is_some_and(path_is_usable_image) || local_index.is_some_and(path_is_usable_image) {
        CoverKind::Art
    } else {
        CoverKind::Monogram
    }
}

/// JPEG / PNG / WebP magic. Used so we never hand a 404 HTML body to `img()`.
pub fn bytes_look_like_image(bytes: &[u8]) -> bool {
    if bytes.len() < MIN_IMAGE_BYTES {
        return false;
    }
    bytes.starts_with(&[0xFF, 0xD8, 0xFF])
        || bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47])
        || (bytes.starts_with(b"RIFF") && bytes.len() > 12 && &bytes[8..12] == b"WEBP")
}

pub fn path_is_usable_image(path: &Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    bytes_look_like_image(&bytes)
}

/// First existing usable image among known Steam library-cache filenames.
pub fn steam_library_cache_cover(steam_root: &Path, app_id: u32) -> Option<PathBuf> {
    let cache = steam_root.join("appcache").join("librarycache");
    let candidates = [
        cache.join(format!("{app_id}_library_600x900.jpg")),
        cache.join(app_id.to_string()).join("library_600x900.jpg"),
        cache.join(format!("{app_id}_library_hero.jpg")),
        cache.join(app_id.to_string()).join("library_hero.jpg"),
        cache.join(format!("{app_id}_header.jpg")),
        cache.join(app_id.to_string()).join("header.jpg"),
    ];
    candidates.into_iter().find(|p| path_is_usable_image(p))
}

/// Extra-store index / install-folder art. Shallow, known names only.
pub fn find_local_cover(install_path: &Path) -> Option<PathBuf> {
    const NAMES: &[&str] = &[
        "cover.jpg",
        "cover.jpeg",
        "cover.png",
        "library.jpg",
        "library.png",
        "poster.jpg",
        "poster.png",
        "vertical.jpg",
        "header.jpg",
        "header.png",
        "capsule.jpg",
    ];
    for name in NAMES {
        let path = install_path.join(name);
        if path_is_usable_image(&path) {
            return Some(path);
        }
    }
    for folder in [".itch", "Support", "__overlay"] {
        let dir = install_path.join(folder);
        if !dir.is_dir() {
            continue;
        }
        for name in NAMES {
            let path = dir.join(name);
            if path_is_usable_image(&path) {
                return Some(path);
            }
        }
    }
    None
}

/// Resolve a file we can decode without hitting the network.
pub fn resolve_local_cover_file(
    app_data_root: &Path,
    title: &LibraryTitle,
    steam_root: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(app_id) = title.steam_app_id() {
        let cached = steam_cover_cache_path(app_data_root, app_id);
        if path_is_usable_image(&cached) {
            return Some(cached);
        }
        if let Some(root) = steam_root {
            if let Some(local) = steam_library_cache_cover(root, app_id) {
                if let Ok(copied) = copy_into_cache(&cached, &local) {
                    return Some(copied);
                }
                return Some(local);
            }
        }
        return None;
    }
    let cached = extra_cover_cache_path(app_data_root, &title.id);
    if path_is_usable_image(&cached) {
        return Some(cached);
    }
    if let Some(local) = find_local_cover(&title.install_path) {
        if let Ok(copied) = copy_into_cache(&cached, &local) {
            return Some(copied);
        }
        return Some(local);
    }
    None
}

fn copy_into_cache(dest: &Path, src: &Path) -> Result<PathBuf, String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create cover cache: {e}"))?;
    }
    std::fs::copy(src, dest).map_err(|e| format!("Could not cache cover: {e}"))?;
    Ok(dest.to_path_buf())
}

/// Download Steam portrait (or hero/header fallback) into the AppData cache.
pub fn fetch_steam_cover_to_cache(app_data_root: &Path, app_id: u32) -> Result<PathBuf, String> {
    let dest = steam_cover_cache_path(app_data_root, app_id);
    if path_is_usable_image(&dest) {
        return Ok(dest);
    }
    let bytes = fetch_first_image(&steam_cover_urls(app_id))?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create cover cache: {e}"))?;
    }
    let tmp = dest.with_extension("jpg.part");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("Could not write cover: {e}"))?;
    std::fs::rename(&tmp, &dest).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Could not finalize cover: {e}")
    })?;
    Ok(dest)
}

fn fetch_first_image(urls: &[String]) -> Result<Vec<u8>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .user_agent(concat!("RusticGU/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("Could not build HTTP client: {e}"))?;
    let mut last = "no cover URLs".to_string();
    for url in urls {
        match client.get(url).send() {
            Ok(resp) if resp.status().is_success() => match resp.bytes() {
                Ok(buf) if bytes_look_like_image(&buf) => return Ok(buf.to_vec()),
                Ok(_) => last = format!("{url}: not an image"),
                Err(err) => last = format!("{url}: {err}"),
            },
            Ok(resp) => last = format!("{url}: HTTP {}", resp.status()),
            Err(err) => last = format!("{url}: {err}"),
        }
    }
    Err(last)
}

/// Decode a cached JPEG/PNG into a GPUI [`RenderImage`] (BGRA frames).
pub fn render_image_from_path(path: &Path) -> Option<Arc<RenderImage>> {
    let dynimg = image::open(path).ok()?;
    let mut rgba = dynimg.to_rgba8();
    for px in rgba.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    let frame = Frame::new(rgba);
    let mut frames: SmallVec<[Frame; 1]> = SmallVec::new();
    frames.push(frame);
    Some(Arc::new(RenderImage::new(frames)))
}

/// Initials for a monogram: up to two letters from significant words.
pub fn monogram_initials(name: &str) -> String {
    let skip = ["the", "a", "an", "of", "and", "in", "to", "for"];
    let words: Vec<&str> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .filter(|w| !skip.contains(&w.to_ascii_lowercase().as_str()))
        .collect();
    let letters: String = words
        .iter()
        .filter_map(|w| w.chars().find(|c| c.is_alphanumeric()))
        .map(|c| c.to_ascii_uppercase())
        .take(2)
        .collect();
    if letters.is_empty() {
        "?".into()
    } else {
        letters
    }
}

/// Extra stores never invent Steam art; Steam without a file is monogram until fetch lands.
#[allow(dead_code)]
pub fn initial_cover_kind(title: &LibraryTitle, resolved: Option<&Path>) -> CoverKind {
    match title.store {
        LibraryStore::Steam => {
            if resolved.is_some_and(path_is_usable_image) {
                CoverKind::Art
            } else {
                CoverKind::Monogram
            }
        }
        LibraryStore::Extra(_) => cover_kind_for(None, resolved),
    }
}

/// Empty placeholder so callers can size a tile without decoding.
#[allow(dead_code)]
pub fn empty_rgba(width: u32, height: u32) -> RgbaImage {
    RgbaImage::new(width.max(1), height.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{extra_title_id, LibraryTitle};
    use std::time::{SystemTime, UNIX_EPOCH};
    use stores::{DiscoveredTitle, StoreId};

    fn stamp() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    }

    fn tiny_jpeg() -> Vec<u8> {
        // 1×1 JPEG. Magic is enough for bytes_look_like_image; decoder tests use PNG.
        let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0];
        bytes.extend_from_slice(&[0u8; 80]);
        bytes
    }

    fn tiny_png() -> Vec<u8> {
        let mut img = RgbaImage::new(2, 3);
        img.put_pixel(0, 0, image::Rgba([10, 20, 30, 255]));
        let mut buf = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    fn steam_title(app_id: u32, name: &str) -> LibraryTitle {
        LibraryTitle {
            id: crate::library::steam_title_id(app_id),
            name: name.into(),
            install_path: PathBuf::from(r"D:\Steam\steamapps\common\Game"),
            store: LibraryStore::Steam,
            launcher_id: Some(app_id.to_string()),
            last_played_unix: None,
            logical_bytes: None,
            on_disk_bytes: None,
            steam_app_id: Some(app_id),
            steam_library_path: None,
            steam_install_dir_name: None,
        }
    }

    #[test]
    fn steam_cover_urls_prefer_portrait_then_hero_then_header() {
        let urls = steam_cover_urls(730);
        assert_eq!(
            urls[0],
            "https://cdn.cloudflare.steamstatic.com/steam/apps/730/library_600x900.jpg"
        );
        assert_eq!(
            urls[1],
            "https://steamcdn-a.akamaihd.net/steam/apps/730/library_600x900.jpg"
        );
        assert!(urls[2].ends_with("/library_hero.jpg"));
        assert!(urls[4].ends_with("/header.jpg"));
        assert!(urls.iter().all(|u| u.contains("/730/")));
        assert_eq!(urls.len(), 6);
    }

    #[test]
    fn steam_cache_path_lives_under_appdata_covers() {
        let root = PathBuf::from("AppData").join("Roaming").join("RusticGU");
        let path = steam_cover_cache_path(&root, 570);
        assert_eq!(path, root.join("covers").join("steam").join("570.jpg"));
        assert!(path.ends_with(Path::new("covers").join("steam").join("570.jpg")));
        assert_eq!(path.file_name().and_then(|n| n.to_str()), Some("570.jpg"));
    }

    #[test]
    fn monogram_when_no_art() {
        assert_eq!(cover_kind_for(None, None), CoverKind::Monogram);
        let missing = PathBuf::from("/no/such/rusticgu/cover.jpg");
        assert_eq!(cover_kind_for(Some(&missing), None), CoverKind::Monogram);
        let title = steam_title(1, "Hades");
        assert_eq!(initial_cover_kind(&title, None), CoverKind::Monogram);
        let mono = Monogram::from_title(&title);
        assert_eq!(mono.initials, "H");
        assert_eq!(mono.store.as_ref(), "Steam");
    }

    #[test]
    fn monogram_initials_skip_articles() {
        assert_eq!(monogram_initials("The Elder Scrolls V"), "ES");
        assert_eq!(monogram_initials("a hat in time"), "HT");
        assert_eq!(monogram_initials(""), "?");
    }

    #[test]
    fn extra_store_uses_index_image_else_monogram() {
        let root = std::env::temp_dir().join(format!(
            "rusticgu-cover-extra-{}-{}",
            std::process::id(),
            stamp()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let discovered =
            DiscoveredTitle::new(StoreId::Gog, "Witcher", &root, Some("1207658913".into()));
        let title = LibraryTitle::from_discovered(discovered);
        assert_eq!(cover_kind_for(None, None), CoverKind::Monogram);
        assert_eq!(initial_cover_kind(&title, None), CoverKind::Monogram);
        assert!(find_local_cover(&root).is_none());

        let png = root.join("cover.png");
        std::fs::write(&png, tiny_png()).unwrap();
        assert_eq!(find_local_cover(&root).as_deref(), Some(png.as_path()));
        assert_eq!(cover_kind_for(None, Some(&png)), CoverKind::Art);
        assert_eq!(initial_cover_kind(&title, Some(&png)), CoverKind::Art);
        assert_eq!(
            extra_title_id(&DiscoveredTitle::new(
                StoreId::Gog,
                "Witcher",
                &root,
                Some("1207658913".into())
            )),
            title.id
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cached_jpeg_is_art_and_not_redownloaded_logic() {
        let root = std::env::temp_dir().join(format!(
            "rusticgu-cover-cache-{}-{}",
            std::process::id(),
            stamp()
        ));
        let dest = steam_cover_cache_path(&root, 440);
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(&dest, tiny_jpeg()).unwrap();
        assert!(path_is_usable_image(&dest));
        assert_eq!(cover_kind_for(Some(&dest), None), CoverKind::Art);
        assert_eq!(
            resolve_local_cover_file(&root, &steam_title(440, "TF2"), None).as_deref(),
            Some(dest.as_path())
        );
        // fetch helper must return the existing file without requiring network
        let again = fetch_steam_cover_to_cache(&root, 440).unwrap();
        assert_eq!(again, dest);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn html_404_body_is_not_an_image() {
        let html = b"<html>404 Not Found</html>........ padding ........";
        assert!(!bytes_look_like_image(html));
        assert!(bytes_look_like_image(&tiny_jpeg()));
        assert!(bytes_look_like_image(&tiny_png()));
    }
}
