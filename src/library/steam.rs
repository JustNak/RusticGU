//! Steam library discovery: registry → libraryfolders.vdf → appmanifest_*.acf.

use std::fs;
use std::path::{Path, PathBuf};

use super::vdf::{lookup_object, lookup_str, parse_vdf, VdfObject};

/// One installed Steam game resolved from an appmanifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamGame {
    pub app_id: u32,
    pub name: String,
    pub install_dir_name: String,
    pub install_path: PathBuf,
    pub library_path: PathBuf,
    /// Logical size from `SizeOnDisk` when present.
    pub logical_bytes: Option<u64>,
    /// On-disk size when cheap to read (directory metadata / compressed size).
    pub on_disk_bytes: Option<u64>,
}

/// Scan every Steam library folder we can find.
pub fn scan_steam_library() -> Result<Vec<SteamGame>, String> {
    let steam_path = steam_path().ok_or_else(|| {
        "Steam is not installed (HKCU SteamPath / typical install folders).".to_string()
    })?;
    let folders = collect_library_folders(&steam_path);
    if folders.is_empty() {
        return Err(format!(
            "No Steam library folders found under {}.",
            steam_path.display()
        ));
    }
    let mut games = Vec::new();
    for folder in folders {
        games.extend(scan_library_folder(&folder));
    }
    games.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
            .then_with(|| a.app_id.cmp(&b.app_id))
    });
    games.dedup_by_key(|g| g.app_id);
    Ok(games)
}

/// HKCU `Software\Valve\Steam\SteamPath`, then common install locations.
pub fn steam_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Some(path) = registry_steam_path() {
            return Some(path);
        }
    }
    typical_steam_paths()
        .into_iter()
        .find(|p| p.join("steam.exe").is_file() || p.join("steamapps").is_dir())
}

#[cfg(windows)]
fn registry_steam_path() -> Option<PathBuf> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_CURRENT_USER, KEY_READ, REG_SZ,
        REG_VALUE_TYPE,
    };

    let subkey = wide(r"Software\Valve\Steam");
    let value = wide("SteamPath");
    unsafe {
        let mut hkey = Default::default();
        let status = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            None,
            KEY_READ,
            &mut hkey,
        );
        if status != ERROR_SUCCESS {
            return None;
        }
        let mut kind = REG_VALUE_TYPE::default();
        let mut bytes = vec![0u8; 1024];
        let mut size = bytes.len() as u32;
        let query = RegQueryValueExW(
            hkey,
            PCWSTR(value.as_ptr()),
            None,
            Some(&mut kind),
            Some(bytes.as_mut_ptr()),
            Some(&mut size),
        );
        let _ = RegCloseKey(hkey);
        if query != ERROR_SUCCESS || kind != REG_SZ {
            return None;
        }
        let u16s: Vec<u16> = bytes[..size as usize]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|u| *u != 0)
            .collect();
        let path = PathBuf::from(String::from_utf16_lossy(&u16s));
        if path.as_os_str().is_empty() {
            None
        } else {
            Some(path)
        }
    }
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn typical_steam_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".steam/steam"));
        paths.push(home.join(".local/share/Steam"));
    }
    paths.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
    paths.push(PathBuf::from(r"C:\Program Files\Steam"));
    paths
}

/// Both `steamapps/libraryfolders.vdf` and `config/libraryfolders.vdf`.
pub fn collect_library_folders(steam_path: &Path) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    push_unique(&mut folders, steam_path.to_path_buf());

    let manifests = [
        steam_path.join("steamapps").join("libraryfolders.vdf"),
        steam_path.join("config").join("libraryfolders.vdf"),
    ];
    for manifest in manifests {
        if let Ok(text) = fs::read_to_string(&manifest) {
            if let Ok(root) = parse_vdf(&text) {
                for path in library_paths_from_vdf(&root) {
                    push_unique(&mut folders, path);
                }
            }
        }
    }
    folders
        .into_iter()
        .filter(|p| p.join("steamapps").is_dir() || p.is_dir())
        .collect()
}

/// Extract library `path` values from a parsed libraryfolders.vdf.
pub fn library_paths_from_vdf(root: &VdfObject) -> Vec<PathBuf> {
    let folders = lookup_object(root, "libraryfolders").unwrap_or(root);
    let mut out = Vec::new();
    for (key, value) in folders {
        if key.eq_ignore_ascii_case("contentstatsid") {
            continue;
        }
        match value {
            super::vdf::VdfValue::String(path) if key.eq_ignore_ascii_case("path") => {
                out.push(PathBuf::from(normalize_vdf_path(path)));
            }
            super::vdf::VdfValue::Object(obj) => {
                if let Some(path) = lookup_str(obj, "path") {
                    out.push(PathBuf::from(normalize_vdf_path(path)));
                }
            }
            _ => {}
        }
    }
    out
}

pub fn scan_library_folder(library_path: &Path) -> Vec<SteamGame> {
    let steamapps = library_path.join("steamapps");
    let Ok(entries) = fs::read_dir(&steamapps) else {
        return Vec::new();
    };
    let mut games = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("appmanifest_") || !name.ends_with(".acf") {
            continue;
        }
        let Ok(text) = fs::read_to_string(entry.path()) else {
            continue;
        };
        if let Some(mut game) = game_from_acf(&text, library_path) {
            fill_cheap_sizes(&mut game);
            games.push(game);
        }
    }
    games
}

/// Parse one `appmanifest_*.acf` into a game record (no disk size walk).
pub fn game_from_acf(text: &str, library_path: &Path) -> Option<SteamGame> {
    let root = parse_vdf(text).ok()?;
    let state = lookup_object(&root, "AppState").unwrap_or(&root);
    let app_id = lookup_str(state, "appid")?.parse().ok()?;
    let name = lookup_str(state, "name")
        .filter(|s| !s.is_empty())
        .unwrap_or("Unknown app")
        .to_string();
    let install_dir_name = lookup_str(state, "installdir")
        .filter(|s| !s.is_empty())?
        .to_string();
    let logical_bytes = lookup_str(state, "SizeOnDisk").and_then(|s| s.parse().ok());
    let install_path = library_path
        .join("steamapps")
        .join("common")
        .join(&install_dir_name);
    Some(SteamGame {
        app_id,
        name,
        install_dir_name,
        install_path,
        library_path: library_path.to_path_buf(),
        logical_bytes,
        on_disk_bytes: None,
    })
}

fn fill_cheap_sizes(game: &mut SteamGame) {
    if !game.install_path.is_dir() {
        return;
    }
    if game.logical_bytes.is_none() {
        if let Some(meta) = cheap_dir_size(&game.install_path) {
            game.logical_bytes = Some(meta);
        }
    }
    game.on_disk_bytes = cheap_on_disk_size(&game.install_path).or(game.logical_bytes);
}

/// Shallow size: immediate children only (cheap; no full tree walk).
fn cheap_dir_size(path: &Path) -> Option<u64> {
    let entries = fs::read_dir(path).ok()?;
    let mut total = 0u64;
    for entry in entries.flatten() {
        if let Ok(meta) = entry.metadata() {
            if meta.is_file() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    Some(total)
}

fn cheap_on_disk_size(path: &Path) -> Option<u64> {
    #[cfg(windows)]
    {
        compressed_dir_size_shallow(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

#[cfg(windows)]
fn compressed_dir_size_shallow(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetCompressedFileSizeW;

    let entries = fs::read_dir(path).ok()?;
    let mut total = 0u64;
    let mut any = false;
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let wide: Vec<u16> = entry
            .path()
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut high = 0u32;
        let low = unsafe { GetCompressedFileSizeW(PCWSTR(wide.as_ptr()), Some(&mut high)) };
        if low == u32::MAX {
            continue;
        }
        any = true;
        total = total.saturating_add(((high as u64) << 32) | low as u64);
    }
    any.then_some(total)
}

fn normalize_vdf_path(path: &str) -> String {
    path.replace("\\\\", "\\")
}

fn push_unique(folders: &mut Vec<PathBuf>, path: PathBuf) {
    let key = normalize_for_cmp(&path);
    if folders
        .iter()
        .any(|existing| normalize_for_cmp(existing) == key)
    {
        return;
    }
    folders.push(path);
}

fn normalize_for_cmp(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_paths_from_both_styles() {
        let src = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"C:\\Steam"
	}
	"1"
	{
		"path"		"E:\\Games\\SteamLibrary"
	}
}
"#;
        let root = parse_vdf(src).unwrap();
        let paths = library_paths_from_vdf(&root);
        assert_eq!(
            paths,
            vec![
                PathBuf::from(r"C:\Steam"),
                PathBuf::from(r"E:\Games\SteamLibrary"),
            ]
        );
    }

    #[test]
    fn acf_resolves_common_installdir() {
        let acf = r#"
"AppState"
{
	"appid"		"730"
	"name"		"Counter-Strike 2"
	"installdir"		"Counter-Strike Global Offensive"
	"SizeOnDisk"		"40000000000"
}
"#;
        let game = game_from_acf(acf, Path::new(r"D:\SteamLibrary")).unwrap();
        assert_eq!(game.app_id, 730);
        assert_eq!(game.name, "Counter-Strike 2");
        assert_eq!(
            normalize_for_cmp(&game.install_path),
            normalize_for_cmp(Path::new(
                r"D:\SteamLibrary\steamapps\common\Counter-Strike Global Offensive"
            ))
        );
        assert_eq!(game.logical_bytes, Some(40_000_000_000));
    }

    #[test]
    fn acf_without_installdir_is_skipped() {
        let acf = r#"
"AppState"
{
	"appid"		"1"
	"name"		"Tool"
}
"#;
        assert!(game_from_acf(acf, Path::new(r"C:\Steam")).is_none());
    }
}
