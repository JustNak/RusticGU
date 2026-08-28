//! Steam library discovery: registry → libraryfolders.vdf → appmanifest_*.acf.

use std::fs;
use std::path::{Path, PathBuf};

#[cfg(test)]
use std::sync::Mutex;

use super::vdf::{lookup_object, lookup_str, parse_vdf, VdfObject};

/// SteamKit `EAppState` bits that mean a title is mid-update / validate / download.
/// `FullyInstalled` (4) and `UpdateRequired` (2) alone are not mid-update.
const STATE_UPDATE_RUNNING: u32 = 256;
const STATE_UPDATE_PAUSED: u32 = 512;
const STATE_UPDATE_STARTED: u32 = 1024;
const STATE_UNINSTALLING: u32 = 2048;
const STATE_BACKUP_RUNNING: u32 = 4096;
const STATE_RECONFIGURING: u32 = 65_536;
const STATE_VALIDATING: u32 = 131_072;
const STATE_ADDING_FILES: u32 = 262_144;
const STATE_PREALLOCATING: u32 = 524_288;
const STATE_DOWNLOADING: u32 = 1_048_576;
const STATE_STAGING: u32 = 2_097_152;
const STATE_COMMITTING: u32 = 4_194_304;
const STATE_UPDATE_STOPPING: u32 = 8_388_608;

const STATE_MID_UPDATE_MASK: u32 = STATE_UPDATE_RUNNING
    | STATE_UPDATE_PAUSED
    | STATE_UPDATE_STARTED
    | STATE_UNINSTALLING
    | STATE_BACKUP_RUNNING
    | STATE_RECONFIGURING
    | STATE_VALIDATING
    | STATE_ADDING_FILES
    | STATE_PREALLOCATING
    | STATE_DOWNLOADING
    | STATE_STAGING
    | STATE_COMMITTING
    | STATE_UPDATE_STOPPING;

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
    ///
    /// Only set when it is comparable to [`Self::logical_bytes`] (same file set).
    /// Never a shallow listing paired with catalog `SizeOnDisk`.
    pub on_disk_bytes: Option<u64>,
    /// Same-scope WOF probe: on-disk of sampled files is ≥5% below their logical size.
    pub compacted: bool,
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
    #[cfg(test)]
    if let Some(path) = test_steam_root() {
        return Some(path);
    }

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

#[cfg(test)]
static TEST_STEAM_ROOT: Mutex<Option<PathBuf>> = Mutex::new(None);

#[cfg(test)]
pub(crate) struct TestSteamRootGuard {
    previous: Option<PathBuf>,
}

#[cfg(test)]
pub(crate) fn set_test_steam_root(path: &Path) -> TestSteamRootGuard {
    let mut root = TEST_STEAM_ROOT
        .lock()
        .expect("test Steam root mutex poisoned");
    let previous = root.replace(path.to_path_buf());
    TestSteamRootGuard { previous }
}

#[cfg(test)]
fn test_steam_root() -> Option<PathBuf> {
    TEST_STEAM_ROOT
        .lock()
        .expect("test Steam root mutex poisoned")
        .clone()
}

#[cfg(test)]
impl Drop for TestSteamRootGuard {
    fn drop(&mut self) {
        let mut root = TEST_STEAM_ROOT
            .lock()
            .expect("test Steam root mutex poisoned");
        *root = self.previous.take();
    }
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
        compacted: false,
    })
}

fn fill_cheap_sizes(game: &mut SteamGame) {
    let (cheap_logical, cheap_on_disk) = cheap_install_sizes(&game.install_path);
    game.compacted = sizes_indicate_compacted(cheap_on_disk, cheap_logical);
    if game.logical_bytes.is_none() {
        game.logical_bytes = cheap_logical;
        game.on_disk_bytes = cheap_on_disk;
    } else {
        game.on_disk_bytes = None;
    }
}

/// True when on-disk size is at least 5% below logical for the **same** file set.
///
/// Catalog `SizeOnDisk` vs a handful of root files must never be passed in:
/// a 4 KB `steam_appid.txt` against a 40 GB ACF size looks like 99% savings.
pub fn sizes_indicate_compacted(on_disk: Option<u64>, logical: Option<u64>) -> bool {
    match (on_disk, logical) {
        (Some(disk), Some(logical)) if logical > 0 => disk.saturating_add(logical / 20) < logical,
        _ => false,
    }
}

/// Max files / depth for the cheap logical vs compressed probe (no full tree walk).
const CHEAP_PROBE_FILES: usize = 48;
const CHEAP_PROBE_DEPTH: usize = 3;

/// Sampled logical / on-disk sizes for an install folder (no full tree walk).
///
/// Both numbers cover the **same** files. Nested folders are sampled so titles
/// whose binaries live under `bin/` / `game/` still classify after a real compact.
pub fn cheap_install_sizes(path: &Path) -> (Option<u64>, Option<u64>) {
    if !path.is_dir() {
        return (None, None);
    }
    let mut logical = 0u64;
    let mut on_disk = 0u64;
    let mut any = false;
    for file in cheap_probe_files(path, CHEAP_PROBE_FILES, CHEAP_PROBE_DEPTH) {
        let Some((file_logical, file_on_disk)) = file_logical_and_on_disk(&file) else {
            continue;
        };
        logical = logical.saturating_add(file_logical);
        on_disk = on_disk.saturating_add(file_on_disk);
        any = true;
    }
    if any {
        (Some(logical), Some(on_disk))
    } else {
        (None, None)
    }
}

fn cheap_probe_files(root: &Path, max_files: usize, max_depth: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn rec(dir: &Path, depth: usize, max_depth: usize, max_files: usize, out: &mut Vec<PathBuf>) {
        if depth > max_depth || out.len() >= max_files {
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut dirs = Vec::new();
        for entry in entries.flatten() {
            if out.len() >= max_files {
                return;
            }
            let path = entry.path();
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_file() {
                out.push(path);
            } else if meta.is_dir() {
                dirs.push(path);
            }
        }
        for nested in dirs {
            rec(&nested, depth + 1, max_depth, max_files, out);
        }
    }
    rec(root, 0, max_depth, max_files, &mut out);
    out
}

fn file_logical_and_on_disk(path: &Path) -> Option<(u64, u64)> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let logical = meta.len();
    let on_disk = file_on_disk_size(path)?;
    Some((logical, on_disk))
}

fn file_on_disk_size(path: &Path) -> Option<u64> {
    #[cfg(windows)]
    {
        compressed_file_size(path)
    }
    #[cfg(not(windows))]
    {
        fs::metadata(path).ok().map(|m| m.len())
    }
}

#[cfg(windows)]
fn compressed_file_size(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetCompressedFileSizeW;

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut high = 0u32;
    let low = unsafe { GetCompressedFileSizeW(PCWSTR(wide.as_ptr()), Some(&mut high)) };
    if low == u32::MAX && high == 0 {
        return None;
    }
    Some(((high as u64) << 32) | u64::from(low))
}

fn normalize_vdf_path(path: &str) -> String {
    path.replace("\\\\", "\\")
}

/// True when `StateFlags` means Steam is actively changing that title.
pub fn state_flags_indicate_update(flags: u32) -> bool {
    flags & STATE_MID_UPDATE_MASK != 0
}

/// `steamapps/downloading/<appid>` exists (directory or file).
pub fn downloading_folder_present(library_path: &Path, app_id: u32) -> bool {
    library_path
        .join("steamapps")
        .join("downloading")
        .join(app_id.to_string())
        .exists()
}

pub fn appmanifest_path(library_path: &Path, app_id: u32) -> PathBuf {
    library_path
        .join("steamapps")
        .join(format!("appmanifest_{app_id}.acf"))
}

/// Read `StateFlags` from an appmanifest ACF body.
pub fn state_flags_from_acf(text: &str) -> Option<u32> {
    let root = parse_vdf(text).ok()?;
    let state = lookup_object(&root, "AppState").unwrap_or(&root);
    lookup_str(state, "StateFlags")?.parse().ok()
}

/// True when Steam is mid-update for this app (downloading folder or StateFlags).
pub fn is_steam_title_updating(library_path: &Path, app_id: u32) -> bool {
    if downloading_folder_present(library_path, app_id) {
        return true;
    }
    let acf = appmanifest_path(library_path, app_id);
    let Ok(text) = fs::read_to_string(acf) else {
        return false;
    };
    state_flags_from_acf(&text).is_some_and(state_flags_indicate_update)
}

/// App id when this install folder is a Steam title that is mid-update.
pub fn steam_updating_app_id(install_path: &Path) -> Option<u32> {
    let (library, app_id) = steam_context_from_install(install_path)?;
    is_steam_title_updating(&library, app_id).then_some(app_id)
}

/// If `install_path` is `…/steamapps/common/<dir>`, resolve its library + appid.
pub fn steam_context_from_install(install_path: &Path) -> Option<(PathBuf, u32)> {
    let common = install_path.parent()?;
    if !common
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("common"))
    {
        return None;
    }
    let steamapps = common.parent()?;
    if !steamapps
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("steamapps"))
    {
        return None;
    }
    let library_path = steamapps.parent()?.to_path_buf();
    let install_dir = install_path.file_name()?.to_string_lossy();
    let entries = fs::read_dir(steamapps).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("appmanifest_") || !name.ends_with(".acf") {
            continue;
        }
        let Ok(text) = fs::read_to_string(entry.path()) else {
            continue;
        };
        let Some(game) = game_from_acf(&text, &library_path) else {
            continue;
        };
        if game.install_dir_name.eq_ignore_ascii_case(&install_dir) {
            return Some((library_path, game.app_id));
        }
    }
    None
}

/// True when this install folder belongs to a Steam title that is mid-update.
pub fn install_is_steam_updating(install_path: &Path) -> bool {
    steam_updating_app_id(install_path).is_some()
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
        assert_eq!(game.on_disk_bytes, None);
        assert!(!game.compacted);
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

    #[test]
    fn state_flags_idle_fully_installed_is_not_updating() {
        assert!(!state_flags_indicate_update(4));
        assert!(!state_flags_indicate_update(4 | 2));
        assert!(!state_flags_indicate_update(0));
    }

    #[test]
    fn state_flags_mid_update_bits_are_detected() {
        assert!(state_flags_indicate_update(STATE_UPDATE_RUNNING));
        assert!(state_flags_indicate_update(STATE_UPDATE_STARTED));
        assert!(state_flags_indicate_update(STATE_DOWNLOADING));
        assert!(state_flags_indicate_update(4 | STATE_UPDATE_RUNNING));
        assert!(state_flags_indicate_update(STATE_VALIDATING));
        assert!(state_flags_from_acf(
            r#"
"AppState"
{
	"appid"		"730"
	"StateFlags"		"256"
	"installdir"		"Counter-Strike Global Offensive"
}
"#
        )
        .is_some_and(state_flags_indicate_update));
        assert!(!state_flags_from_acf(
            r#"
"AppState"
{
	"appid"		"730"
	"StateFlags"		"4"
	"installdir"		"Counter-Strike Global Offensive"
}
"#
        )
        .is_some_and(state_flags_indicate_update));
    }

    #[test]
    fn downloading_folder_and_acf_flags_block_compact() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let library = std::env::temp_dir().join(format!(
            "rusticgu-steam-upd-{}-{}",
            std::process::id(),
            stamp
        ));
        let common = library.join("steamapps").join("common").join("FooGame");
        std::fs::create_dir_all(&common).unwrap();
        std::fs::write(common.join("game.exe"), b"exe").unwrap();

        let idle_acf = r#"
"AppState"
{
	"appid"		"4242"
	"name"		"Foo Game"
	"StateFlags"		"4"
	"installdir"		"FooGame"
}
"#;
        std::fs::write(appmanifest_path(&library, 4242), idle_acf).unwrap();
        assert!(!is_steam_title_updating(&library, 4242));
        assert!(!install_is_steam_updating(&common));
        assert!(!downloading_folder_present(&library, 4242));

        let downloading = library.join("steamapps").join("downloading").join("4242");
        std::fs::create_dir_all(&downloading).unwrap();
        assert!(downloading_folder_present(&library, 4242));
        assert!(is_steam_title_updating(&library, 4242));
        assert!(install_is_steam_updating(&common));
        let _ = std::fs::remove_dir_all(&downloading);
        assert!(!is_steam_title_updating(&library, 4242));

        let updating_acf = r#"
"AppState"
{
	"appid"		"4242"
	"name"		"Foo Game"
	"StateFlags"		"1048576"
	"installdir"		"FooGame"
}
"#;
        std::fs::write(appmanifest_path(&library, 4242), updating_acf).unwrap();
        assert!(is_steam_title_updating(&library, 4242));
        assert!(install_is_steam_updating(&common));

        let _ = std::fs::remove_dir_all(&library);
    }

    #[test]
    fn is_steam_title_updating_when_stateflags_updating() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let library = std::env::temp_dir().join(format!(
            "rusticgu-flags-upd-{}-{}",
            std::process::id(),
            stamp
        ));
        std::fs::create_dir_all(library.join("steamapps")).unwrap();
        std::fs::write(
            appmanifest_path(&library, 1026),
            r#"
"AppState"
{
	"appid"		"1026"
	"StateFlags"		"1026"
	"installdir"		"Patching"
}
"#,
        )
        .unwrap();
        assert!(is_steam_title_updating(&library, 1026));
        let _ = std::fs::remove_dir_all(&library);
    }

    #[test]
    fn is_steam_title_updating_when_downloading_folder_present() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let library =
            std::env::temp_dir().join(format!("rusticgu-dl-upd-{}-{}", std::process::id(), stamp));
        std::fs::create_dir_all(library.join("steamapps").join("downloading").join("77")).unwrap();
        assert!(downloading_folder_present(&library, 77));
        assert!(is_steam_title_updating(&library, 77));
        let _ = std::fs::remove_dir_all(&library);
    }

    #[test]
    fn is_steam_title_updating_false_when_fully_installed() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let library = std::env::temp_dir().join(format!(
            "rusticgu-idle-upd-{}-{}",
            std::process::id(),
            stamp
        ));
        std::fs::create_dir_all(library.join("steamapps")).unwrap();
        std::fs::write(
            appmanifest_path(&library, 4),
            r#"
"AppState"
{
	"appid"		"4"
	"StateFlags"		"4"
	"installdir"		"Ready"
}
"#,
        )
        .unwrap();
        assert!(!is_steam_title_updating(&library, 4));
        assert!(!downloading_folder_present(&library, 4));
        let _ = std::fs::remove_dir_all(&library);
    }

    fn temp_install(tag: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "rusticgu-cheap-{tag}-{}-{}",
            std::process::id(),
            stamp
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn steam_game_at(install_path: PathBuf, catalog: Option<u64>) -> SteamGame {
        SteamGame {
            app_id: 730,
            name: "Counter-Strike 2".into(),
            install_dir_name: "CS2".into(),
            library_path: install_path.clone(),
            install_path,
            logical_bytes: catalog,
            on_disk_bytes: None,
            compacted: false,
        }
    }

    #[test]
    fn sizes_indicate_compacted_needs_five_percent_same_scope() {
        assert!(sizes_indicate_compacted(Some(4), Some(10)));
        assert!(!sizes_indicate_compacted(Some(20), Some(20)));
        assert!(!sizes_indicate_compacted(Some(19), Some(20)));
        assert!(sizes_indicate_compacted(Some(18), Some(20)));
        assert!(!sizes_indicate_compacted(None, Some(10)));
        assert!(!sizes_indicate_compacted(Some(126_000_000), None));
        assert!(!sizes_indicate_compacted(Some(0), Some(0)));
        assert!(!sizes_indicate_compacted(Some(1), Some(0)));
        assert!(sizes_indicate_compacted(Some(4_096), Some(40_000_000_000)));
    }

    #[test]
    fn catalog_size_is_not_mixed_with_shallow_on_disk() {
        let dir = temp_install("catalog");
        fs::write(dir.join("steam_appid.txt"), b"730").unwrap();
        fs::write(dir.join("game.exe"), vec![0u8; 64 * 1024]).unwrap();

        let mut game = steam_game_at(dir.clone(), Some(40_000_000_000));
        fill_cheap_sizes(&mut game);

        assert_eq!(game.logical_bytes, Some(40_000_000_000));
        assert_eq!(
            game.on_disk_bytes, None,
            "sampled on-disk must not pair with ACF SizeOnDisk"
        );
        assert!(
            !game.compacted,
            "uncompressed install files must not count as compacted against catalog size"
        );

        let title = crate::library::LibraryTitle::from_steam(game, None);
        assert!(!title.is_compacted());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cheap_probe_includes_nested_files_on_the_same_set() {
        let dir = temp_install("nested");
        fs::create_dir_all(dir.join("bin")).unwrap();
        fs::write(dir.join("bin").join("game.dll"), vec![0u8; 32 * 1024]).unwrap();

        let (logical, on_disk) = cheap_install_sizes(&dir);
        assert!(logical.unwrap() >= 32 * 1024);
        assert!(on_disk.is_some());
        assert!(
            !sizes_indicate_compacted(on_disk, logical),
            "uncompressed nested files must not look compacted"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_catalog_keeps_comparable_cheap_pair() {
        let dir = temp_install("nocate");
        fs::write(dir.join("game.exe"), vec![0u8; 8192]).unwrap();

        let mut game = steam_game_at(dir.clone(), None);
        fill_cheap_sizes(&mut game);

        assert!(game.logical_bytes.is_some());
        assert!(game.on_disk_bytes.is_some());
        assert!(!game.compacted);
        let logical = game.logical_bytes.unwrap();
        let on_disk = game.on_disk_bytes.unwrap();
        assert!(
            on_disk.saturating_add(logical / 20) >= logical,
            "uncompressed cheap pair must not trip the 5% heuristic"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
