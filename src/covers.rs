//! Steam / extra-store cover art: local cache first, hashed CDN, monogram fallback.
//!
//! Never paints a broken-image card. Never scrapes store HTML.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui::{RenderImage, SharedString};
use image::{Frame, RgbaImage};
use smallvec::SmallVec;

use crate::library::{LibraryStore, LibraryTitle};

/// Modern hashed / unhashed Steam portrait host.
pub const STEAM_CDN_SHARED: &str = "https://shared.steamstatic.com/store_item_assets/steam/apps";
/// Legacy hosts — many 2025–26 titles 404 here without a hash. Tried last.
pub const STEAM_CDN_LEGACY_CLOUDFLARE: &str = "https://cdn.cloudflare.steamstatic.com/steam/apps";
pub const STEAM_CDN_LEGACY_AKAMAI: &str = "https://steamcdn-a.akamaihd.net/steam/apps";

/// assetcache.vdf field for the 600×900 library grid.
const ASSET_GRID_FIELD: &str = "0f";
const PORTRAIT_FILE: &str = "library_600x900.jpg";
const FETCH_TIMEOUT: Duration = Duration::from_secs(12);
const MIN_IMAGE_BYTES: usize = 64;
const PORTRAIT_W: u32 = 600;
const PORTRAIT_H: u32 = 900;

/// How a title should be drawn on the gallery wall.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverKind {
    Art,
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

/// Steam portrait URL order: hashed shared CDN, unhashed shared, then legacy.
pub fn steam_cover_urls(app_id: u32, hashes: &[String]) -> Vec<String> {
    let mut urls = Vec::new();
    for hash in hashes {
        let hash = hash.trim().trim_matches('/');
        if hash.is_empty() {
            continue;
        }
        urls.push(format!(
            "{STEAM_CDN_SHARED}/{app_id}/{hash}/{PORTRAIT_FILE}"
        ));
    }
    urls.push(format!("{STEAM_CDN_SHARED}/{app_id}/{PORTRAIT_FILE}"));
    urls.push(format!(
        "{STEAM_CDN_LEGACY_CLOUDFLARE}/{app_id}/{PORTRAIT_FILE}"
    ));
    urls.push(format!(
        "{STEAM_CDN_LEGACY_AKAMAI}/{app_id}/{PORTRAIT_FILE}"
    ));
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

#[allow(dead_code)]
pub fn cover_kind_for(cached: Option<&Path>, local_index: Option<&Path>) -> CoverKind {
    if cached.is_some_and(path_is_usable_image) || local_index.is_some_and(path_is_usable_image) {
        CoverKind::Art
    } else {
        CoverKind::Monogram
    }
}

pub fn bytes_look_like_image(bytes: &[u8]) -> bool {
    if bytes.len() < MIN_IMAGE_BYTES {
        return false;
    }
    bytes.starts_with(&[0xFF, 0xD8, 0xFF])
        || bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47])
        || bytes.starts_with(&[0x00, 0x00, 0x01, 0x00])
        || (bytes.starts_with(b"RIFF") && bytes.len() > 12 && &bytes[8..12] == b"WEBP")
}

pub fn path_is_usable_image(path: &Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    bytes_look_like_image(&bytes)
}

/// Hashes recorded for this appid in `appcache/librarycache/assetcache.vdf`.
pub fn steam_asset_hashes(steam_root: &Path, app_id: u32) -> Vec<String> {
    let vdf = steam_root
        .join("appcache")
        .join("librarycache")
        .join("assetcache.vdf");
    let Ok(bytes) = std::fs::read(&vdf) else {
        return Vec::new();
    };
    hashes_from_assetcache_bytes(&bytes, app_id)
}

pub fn hashes_from_assetcache_bytes(bytes: &[u8], app_id: u32) -> Vec<String> {
    let Some(root) = parse_binary_vdf(bytes) else {
        return Vec::new();
    };
    let apps = assetcache_apps(&root);
    let Some(entry) = apps.get(&app_id.to_string()) else {
        return Vec::new();
    };
    let mut hashes = Vec::new();
    if let Some(rel) = entry.get(ASSET_GRID_FIELD) {
        if let Some(hash) = hash_from_relative_path(rel) {
            push_unique(&mut hashes, hash);
        }
    }
    for value in entry.values() {
        if let Some(hash) = hash_from_relative_path(value) {
            push_unique(&mut hashes, hash);
        }
    }
    hashes
}

/// Local Steam portrait: assetcache path, then known 600×900 filenames. No landscape.
pub fn steam_library_cache_cover(steam_root: &Path, app_id: u32) -> Option<PathBuf> {
    let cache = steam_root.join("appcache").join("librarycache");
    if let Some(rel) = assetcache_grid_rel(steam_root, app_id) {
        let path = cache.join(app_id.to_string()).join(rel);
        if path_is_usable_image(&path) {
            return Some(path);
        }
    }
    let candidates = [
        cache.join(app_id.to_string()).join(PORTRAIT_FILE),
        cache.join(format!("{app_id}_{PORTRAIT_FILE}")),
    ];
    if let Some(found) = candidates.into_iter().find(|p| path_is_usable_image(p)) {
        return Some(found);
    }
    walk_portrait(&cache.join(app_id.to_string()), 0)
}

fn assetcache_grid_rel(steam_root: &Path, app_id: u32) -> Option<String> {
    let vdf = steam_root
        .join("appcache")
        .join("librarycache")
        .join("assetcache.vdf");
    let bytes = std::fs::read(vdf).ok()?;
    let root = parse_binary_vdf(&bytes)?;
    let apps = assetcache_apps(&root);
    apps.get(&app_id.to_string())?
        .get(ASSET_GRID_FIELD)
        .cloned()
}

fn walk_portrait(dir: &Path, depth: u32) -> Option<PathBuf> {
    if depth > 2 || !dir.is_dir() {
        return None;
    }
    let direct = dir.join(PORTRAIT_FILE);
    if path_is_usable_image(&direct) {
        return Some(direct);
    }
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = walk_portrait(&path, depth + 1) {
                return Some(found);
            }
        }
    }
    None
}

/// Extra-store art: itch `cover_url` is fetched separately. Others: `.ico` only.
/// Landscape capsules / headers are ignored.
pub fn find_local_cover(install_path: &Path) -> Option<PathBuf> {
    const NAMES: &[&str] = &["icon.ico", "game.ico", "app.ico", "cover.ico"];
    for name in NAMES {
        let path = install_path.join(name);
        if path_is_usable_image(&path) {
            return Some(path);
        }
    }
    if let Ok(entries) = std::fs::read_dir(install_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("ico"))
                && path_is_usable_image(&path)
            {
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

/// Download Steam 600×900 into the AppData cache. Local librarycache wins.
pub fn fetch_steam_cover_to_cache(
    app_data_root: &Path,
    steam_root: Option<&Path>,
    app_id: u32,
) -> Result<PathBuf, String> {
    let dest = steam_cover_cache_path(app_data_root, app_id);
    if path_is_usable_image(&dest) {
        return Ok(dest);
    }
    if let Some(root) = steam_root {
        if let Some(local) = steam_library_cache_cover(root, app_id) {
            return copy_into_cache(&dest, &local);
        }
    }
    let hashes = steam_root
        .map(|root| steam_asset_hashes(root, app_id))
        .unwrap_or_default();
    let bytes = fetch_first_image(&steam_cover_urls(app_id, &hashes))?;
    write_cache_bytes(&dest, &bytes)
}

/// Fetch a direct image URL (itch cover) and letterbox into 2:3.
pub fn fetch_url_cover_to_cache(
    dest: &Path,
    url: &str,
    letterbox: bool,
) -> Result<PathBuf, String> {
    if path_is_usable_image(dest) {
        return Ok(dest.to_path_buf());
    }
    if !url_looks_like_direct_image(url) {
        return Err("not a direct image URL".into());
    }
    let bytes = fetch_first_image(&[url.to_string()])?;
    if !letterbox {
        return write_cache_bytes(dest, &bytes);
    }
    let dynimg = image::load_from_memory(&bytes).map_err(|e| format!("decode cover: {e}"))?;
    let framed = letterbox_to_portrait(dynimg);
    write_png_cache(dest, &framed)
}

pub fn url_looks_like_direct_image(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return false;
    }
    if lower.contains("store.steampowered.com")
        || lower.contains("itch.io/game")
        || lower.contains("/html")
    {
        return false;
    }
    lower.contains("img.itch.zone")
        || lower.contains("steamstatic.com")
        || lower.contains("akamaihd.net")
        || lower.contains("itch.zone")
        || lower.split('?').next().is_some_and(|p| {
            p.ends_with(".jpg")
                || p.ends_with(".jpeg")
                || p.ends_with(".png")
                || p.ends_with(".webp")
        })
}

/// itch Fetch.Caves `coverUrl` / `stillCoverUrl` for one installed title.
pub fn itch_cover_url_from_sidecar(
    text: &str,
    launcher_id: Option<&str>,
    install: &Path,
) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let mut caves = Vec::new();
    if let Some(arr) = v.get("caves").and_then(|x| x.as_array()) {
        caves.extend(arr.iter());
    }
    if let Some(arr) = v.get("items").and_then(|x| x.as_array()) {
        for item in arr {
            if let Some(cave) = item.get("cave") {
                caves.push(cave);
            } else {
                caves.push(item);
            }
        }
    }
    let install_key = normalize_path(install);
    for cave in caves {
        let id_hit = launcher_id.is_some_and(|want| cave_id(cave).as_deref() == Some(want));
        let path_hit = cave_install(cave).is_some_and(|p| normalize_path(&p) == install_key);
        if !id_hit && !path_hit {
            continue;
        }
        let game = cave.get("game").unwrap_or(cave);
        return game
            .get("stillCoverUrl")
            .or_else(|| game.get("still_cover_url"))
            .or_else(|| game.get("coverUrl"))
            .or_else(|| game.get("cover_url"))
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
    }
    None
}

pub fn attach_itch_cover_urls(titles: &mut [LibraryTitle], itch_config: Option<&Path>) {
    let Some(dir) = itch_config else {
        return;
    };
    let text = ["fetch_caves.json", "caves.json"]
        .into_iter()
        .find_map(|name| std::fs::read_to_string(dir.join(name)).ok());
    let Some(text) = text else {
        return;
    };
    for title in titles.iter_mut() {
        if !matches!(title.store, LibraryStore::Extra(stores::StoreId::Itch)) {
            continue;
        }
        if title.cover_url.is_some() {
            continue;
        }
        title.cover_url =
            itch_cover_url_from_sidecar(&text, title.launcher_id.as_deref(), &title.install_path);
    }
}

fn cave_id(cave: &serde_json::Value) -> Option<String> {
    cave.get("id")
        .and_then(|v| match v {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .or_else(|| {
            cave.get("game")
                .and_then(|g| g.get("id"))
                .and_then(|v| match v {
                    serde_json::Value::String(s) => Some(s.clone()),
                    serde_json::Value::Number(n) => Some(n.to_string()),
                    _ => None,
                })
        })
}

fn cave_install(cave: &serde_json::Value) -> Option<PathBuf> {
    cave.get("installInfo")
        .or_else(|| cave.get("install_info"))
        .and_then(|i| i.get("installFolder").or_else(|| i.get("install_folder")))
        .and_then(|v| v.as_str())
        .or_else(|| cave.get("installFolder").and_then(|v| v.as_str()))
        .or_else(|| cave.get("install_folder").and_then(|v| v.as_str()))
        .or_else(|| cave.get("installPath").and_then(|v| v.as_str()))
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

fn write_cache_bytes(dest: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create cover cache: {e}"))?;
    }
    let tmp = dest.with_extension("part");
    std::fs::write(&tmp, bytes).map_err(|e| format!("Could not write cover: {e}"))?;
    std::fs::rename(&tmp, dest).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Could not finalize cover: {e}")
    })?;
    Ok(dest.to_path_buf())
}

fn write_png_cache(dest: &Path, img: &RgbaImage) -> Result<PathBuf, String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create cover cache: {e}"))?;
    }
    let tmp = dest.with_extension("part");
    let mut buf = Vec::new();
    image::DynamicImage::ImageRgba8(img.clone())
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| format!("encode cover: {e}"))?;
    std::fs::write(&tmp, &buf).map_err(|e| format!("Could not write cover: {e}"))?;
    std::fs::rename(&tmp, dest).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Could not finalize cover: {e}")
    })?;
    Ok(dest.to_path_buf())
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

/// Fit any raster into a 2:3 portrait canvas (letterbox / pillarbox).
pub fn letterbox_to_portrait(img: image::DynamicImage) -> RgbaImage {
    let src = img.to_rgba8();
    let (sw, sh) = src.dimensions();
    let sw = sw.max(1);
    let sh = sh.max(1);
    let scale = (PORTRAIT_W as f32 / sw as f32).min(PORTRAIT_H as f32 / sh as f32);
    let nw = ((sw as f32) * scale).round().max(1.0) as u32;
    let nh = ((sh as f32) * scale).round().max(1.0) as u32;
    let resized = image::imageops::resize(&src, nw, nh, image::imageops::FilterType::Triangle);
    let mut canvas = RgbaImage::from_pixel(PORTRAIT_W, PORTRAIT_H, image::Rgba([18, 22, 26, 255]));
    let x = (PORTRAIT_W - nw) / 2;
    let y = (PORTRAIT_H - nh) / 2;
    image::imageops::overlay(&mut canvas, &resized, x.into(), y.into());
    canvas
}

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

#[allow(dead_code)]
pub fn empty_rgba(width: u32, height: u32) -> RgbaImage {
    RgbaImage::new(width.max(1), height.max(1))
}

fn hash_from_relative_path(rel: &str) -> Option<String> {
    let rel = rel.replace('\\', "/");
    let mut parts = rel.split('/');
    let first = parts.next()?.trim();
    let rest = parts.next();
    if rest.is_some() && looks_like_asset_hash(first) {
        return Some(first.to_ascii_lowercase());
    }
    None
}

fn looks_like_asset_hash(s: &str) -> bool {
    s.len() >= 8 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn push_unique(out: &mut Vec<String>, value: String) {
    if !out.iter().any(|v| v == &value) {
        out.push(value);
    }
}

#[derive(Debug, Clone)]
enum BinVal {
    Str(String),
    Obj(BTreeMap<String, BinVal>),
}

fn parse_binary_vdf(bytes: &[u8]) -> Option<BTreeMap<String, BinVal>> {
    let mut i = 0usize;
    let mut root = BTreeMap::new();
    while i < bytes.len() {
        match read_bin_entry(bytes, &mut i)? {
            BinRead::End => break,
            BinRead::Pair(k, v) => {
                root.insert(k, v);
            }
        }
    }
    if root.is_empty() {
        None
    } else {
        Some(root)
    }
}

enum BinRead {
    End,
    Pair(String, BinVal),
}

fn read_bin_entry(bytes: &[u8], i: &mut usize) -> Option<BinRead> {
    if *i >= bytes.len() {
        return None;
    }
    let ty = bytes[*i];
    *i += 1;
    if ty == 0x08 {
        return Some(BinRead::End);
    }
    let key = read_cstring(bytes, i)?;
    let val = match ty {
        0x00 => {
            let mut child = BTreeMap::new();
            loop {
                match read_bin_entry(bytes, i)? {
                    BinRead::End => break,
                    BinRead::Pair(k, v) => {
                        child.insert(k, v);
                    }
                }
            }
            BinVal::Obj(child)
        }
        0x01 => BinVal::Str(read_cstring(bytes, i)?),
        0x02 => {
            if *i + 4 > bytes.len() {
                return None;
            }
            let n = i32::from_le_bytes(bytes[*i..*i + 4].try_into().ok()?);
            *i += 4;
            BinVal::Str(n.to_string())
        }
        0x07 => {
            if *i + 8 > bytes.len() {
                return None;
            }
            let n = u64::from_le_bytes(bytes[*i..*i + 8].try_into().ok()?);
            *i += 8;
            BinVal::Str(n.to_string())
        }
        _ => return None,
    };
    Some(BinRead::Pair(key, val))
}

fn read_cstring(bytes: &[u8], i: &mut usize) -> Option<String> {
    let start = *i;
    while *i < bytes.len() && bytes[*i] != 0 {
        *i += 1;
    }
    if *i >= bytes.len() {
        return None;
    }
    let s = String::from_utf8_lossy(&bytes[start..*i]).into_owned();
    *i += 1;
    Some(s)
}

fn assetcache_apps(root: &BTreeMap<String, BinVal>) -> BTreeMap<String, BTreeMap<String, String>> {
    if looks_like_apps_map(root) {
        return flatten_apps(root);
    }
    for value in root.values() {
        if let BinVal::Obj(inner) = value {
            if looks_like_apps_map(inner) {
                return flatten_apps(inner);
            }
            for nested in inner.values() {
                if let BinVal::Obj(apps) = nested {
                    if looks_like_apps_map(apps) {
                        return flatten_apps(apps);
                    }
                }
            }
        }
    }
    BTreeMap::new()
}

fn looks_like_apps_map(map: &BTreeMap<String, BinVal>) -> bool {
    map.iter().any(|(k, v)| {
        k.chars().all(|c| c.is_ascii_digit())
            && matches!(
                v,
                BinVal::Obj(fields) if fields.values().any(|x| matches!(x, BinVal::Str(_)))
            )
    })
}

fn flatten_apps(map: &BTreeMap<String, BinVal>) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for (k, v) in map {
        if let BinVal::Obj(fields) = v {
            let mut flat = BTreeMap::new();
            for (fk, fv) in fields {
                if let BinVal::Str(s) = fv {
                    flat.insert(fk.clone(), s.clone());
                }
            }
            out.insert(k.clone(), flat);
        }
    }
    out
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
            compacted: false,
            steam_app_id: Some(app_id),
            steam_library_path: None,
            steam_install_dir_name: None,
            cover_url: None,
        }
    }

    fn bin_str(key: &str, val: &str) -> Vec<u8> {
        let mut out = vec![0x01];
        out.extend(key.as_bytes());
        out.push(0);
        out.extend(val.as_bytes());
        out.push(0);
        out
    }

    fn bin_obj(key: &str, body: &[u8]) -> Vec<u8> {
        let mut out = vec![0x00];
        out.extend(key.as_bytes());
        out.push(0);
        out.extend_from_slice(body);
        out.push(0x08);
        out
    }

    #[test]
    fn steam_cover_urls_prefer_hashed_shared_cdn() {
        let urls = steam_cover_urls(730, &["deadbeefcafebabe".into()]);
        assert_eq!(
            urls[0],
            "https://shared.steamstatic.com/store_item_assets/steam/apps/730/deadbeefcafebabe/library_600x900.jpg"
        );
        assert_eq!(
            urls[1],
            "https://shared.steamstatic.com/store_item_assets/steam/apps/730/library_600x900.jpg"
        );
        assert!(urls
            .iter()
            .any(|u| u.contains("cdn.cloudflare.steamstatic.com")));
        assert_eq!(
            urls.iter().filter(|u| u.contains("library_hero")).count(),
            0
        );
        assert!(urls
            .iter()
            .all(|u| u.contains("/730/") && u.ends_with(PORTRAIT_FILE)));
    }

    #[test]
    fn assetcache_binary_yields_grid_hash() {
        let app = bin_obj(
            "730",
            &bin_str("0f", "deadbeefcafebabe/library_600x900.jpg"),
        );
        let cache = bin_obj("0", &app);
        let mut root = bin_obj("librarycache", &cache);
        root.push(0x08);
        let hashes = hashes_from_assetcache_bytes(&root, 730);
        assert_eq!(hashes, vec!["deadbeefcafebabe".to_string()]);
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
    fn extra_store_ignores_landscape_capsule() {
        let root = std::env::temp_dir().join(format!(
            "rusticgu-cover-extra-{}-{}",
            std::process::id(),
            stamp()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let discovered =
            DiscoveredTitle::new(StoreId::Gog, "Witcher", &root, Some("1207658913".into()));
        let title = LibraryTitle::from_discovered(discovered);
        std::fs::write(root.join("capsule.jpg"), tiny_jpeg()).unwrap();
        std::fs::write(root.join("header.jpg"), tiny_jpeg()).unwrap();
        assert!(
            find_local_cover(&root).is_none(),
            "landscape must not be used"
        );
        assert_eq!(initial_cover_kind(&title, None), CoverKind::Monogram);

        let ico = root.join("icon.ico");
        std::fs::write(&ico, {
            let mut b = vec![0x00, 0x00, 0x01, 0x00];
            b.extend_from_slice(&[0u8; 80]);
            b
        })
        .unwrap();
        assert_eq!(find_local_cover(&root).as_deref(), Some(ico.as_path()));
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
    fn itch_cover_url_from_caves_json() {
        let json = r#"{
            "caves": [{
                "id": "cave-1",
                "installFolder": "D:\\itch\\Celeste",
                "game": {
                    "title": "Celeste",
                    "coverUrl": "https://img.itch.zone/celeste.png",
                    "stillCoverUrl": "https://img.itch.zone/celeste-still.png"
                }
            }]
        }"#;
        let url = itch_cover_url_from_sidecar(json, Some("cave-1"), Path::new(r"D:\itch\Celeste"));
        assert_eq!(
            url.as_deref(),
            Some("https://img.itch.zone/celeste-still.png")
        );
        assert!(url_looks_like_direct_image(url.as_deref().unwrap()));
        assert!(!url_looks_like_direct_image("https://itch.io/game/celeste"));
    }

    #[test]
    fn letterbox_itch_cover_is_2_by_3() {
        let src = image::DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            315,
            250,
            image::Rgba([9, 8, 7, 255]),
        ));
        let out = letterbox_to_portrait(src);
        assert_eq!(out.dimensions(), (600, 900));
        assert_eq!(out.width() * 3, out.height() * 2);
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
        let again = fetch_steam_cover_to_cache(&root, None, 440).unwrap();
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
