//! Compact / uncompact a game folder via WOF `compact /EXE`.

use std::path::{Path, PathBuf};

use crate::settings::CompactAlgorithm;

use crate::library::steam_updating_app_id;

use super::command::CompactOp;
use super::skip::{auto_excluded_title, collect_inventory, tree_contains_dstorage};

/// Typical XPRESS8K ratio used for dry-run estimates (conservative).
const XPRESS8K_RATIO: f64 = 0.62;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactRefuse {
    ReFs,
    WindowsApps,
    RunningExecutable { path: PathBuf },
    DirectStorage { override_allowed: bool },
    AutoExcluded { title: String },
    SteamUpdating { app_id: u32 },
    UnsupportedOs,
    NotNtfs { filesystem: String },
    MissingFolder,
}

impl std::fmt::Display for CompactRefuse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReFs => write!(f, "Refusing to compact a ReFS volume."),
            Self::WindowsApps => {
                write!(f, "Refusing to compact a WindowsApps / Store install.")
            }
            Self::RunningExecutable { path } => write!(
                f,
                "A running executable is inside this folder:\n{}",
                path.display()
            ),
            Self::DirectStorage { override_allowed } => {
                if *override_allowed {
                    write!(
                        f,
                        "dstorage.dll or dstoragecore.dll is present; override is enabled."
                    )
                } else {
                    write!(
                        f,
                        "This install includes dstorage.dll or dstoragecore.dll (DirectStorage). Compact is blocked unless you enable the override in Settings → General."
                    )
                }
            }
            Self::AutoExcluded { title } => {
                write!(f, "{title} is auto-excluded from compact.")
            }
            Self::SteamUpdating { app_id } => write!(
                f,
                "Steam is updating this title (app {app_id}). Compact is blocked until the update finishes."
            ),
            Self::UnsupportedOs => {
                write!(
                    f,
                    "WOF Compact /EXE requires Windows 10 version 1607 or later."
                )
            }
            Self::NotNtfs { filesystem } => {
                write!(f, "WOF Compact /EXE requires NTFS (found {filesystem}).")
            }
            Self::MissingFolder => write!(f, "The game folder is missing."),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactEstimate {
    pub file_count: usize,
    pub skipped_count: usize,
    pub logical_bytes: u64,
    pub estimated_on_disk_bytes: u64,
    pub has_dstorage: bool,
}

impl CompactEstimate {
    pub fn saved_bytes(&self) -> u64 {
        self.logical_bytes
            .saturating_sub(self.estimated_on_disk_bytes)
    }
}

/// Measured logical vs on-disk size of compactable files in an install.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CompactSizeSnapshot {
    pub logical_bytes: u64,
    pub on_disk_bytes: u64,
    pub file_count: usize,
}

impl CompactSizeSnapshot {
    pub fn saved_bytes(self) -> u64 {
        self.logical_bytes.saturating_sub(self.on_disk_bytes)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompactProgress {
    pub processed: usize,
    pub total: usize,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactResult {
    pub ok: bool,
    pub message: String,
}

/// Dry-run: walk the tree, apply the skip list, estimate on-disk size.
pub fn estimate_compact(
    root: &Path,
    algorithm: CompactAlgorithm,
) -> Result<CompactEstimate, String> {
    estimate_compact_with(root, algorithm.for_live_library())
}

/// Estimate using the algorithm as given (Shelf may pass LZX).
pub fn estimate_compact_with(
    root: &Path,
    algorithm: CompactAlgorithm,
) -> Result<CompactEstimate, String> {
    if !root.is_dir() {
        return Err("Game folder is missing.".into());
    }
    let inv = collect_inventory(root);
    let ratio = estimate_ratio(algorithm);
    let estimated_on_disk_bytes = (inv.logical_bytes as f64 * ratio).round() as u64;
    Ok(CompactEstimate {
        file_count: inv.included.len(),
        skipped_count: inv.skipped_count(),
        logical_bytes: inv.logical_bytes,
        estimated_on_disk_bytes,
        has_dstorage: inv.has_dstorage,
    })
}

pub(crate) fn estimate_ratio(algorithm: CompactAlgorithm) -> f64 {
    match algorithm {
        CompactAlgorithm::Xpress4k => 0.70,
        CompactAlgorithm::Xpress8k => XPRESS8K_RATIO,
        CompactAlgorithm::Xpress16k => 0.55,
        CompactAlgorithm::Xpress => 0.68,
        CompactAlgorithm::Lzx => 0.48,
    }
}

/// Walk compactable files and sum logical + on-disk bytes (same skip list as apply).
pub fn measure_compact_sizes(root: &Path) -> CompactSizeSnapshot {
    let inv = collect_inventory(root);
    let mut on_disk_bytes = 0u64;
    for path in &inv.included {
        let logical = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let on_disk = file_on_disk_bytes(path).unwrap_or(logical);
        on_disk_bytes = on_disk_bytes.saturating_add(on_disk);
    }
    CompactSizeSnapshot {
        logical_bytes: inv.logical_bytes,
        on_disk_bytes,
        file_count: inv.included.len(),
    }
}

fn file_on_disk_bytes(path: &Path) -> Option<u64> {
    #[cfg(windows)]
    {
        windows_file_on_disk_bytes(path)
    }
    #[cfg(not(windows))]
    {
        std::fs::metadata(path).ok().map(|m| m.len())
    }
}

#[cfg(windows)]
fn windows_file_on_disk_bytes(path: &Path) -> Option<u64> {
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

/// Preflight guards. `allow_dstorage` lets the user override the dstorage.dll warning.
pub fn preflight(root: &Path, allow_dstorage: bool) -> Result<(), CompactRefuse> {
    if !root.exists() {
        return Err(CompactRefuse::MissingFolder);
    }
    if let Some(title) = auto_excluded_title(root) {
        return Err(CompactRefuse::AutoExcluded { title });
    }
    if is_windows_apps_path(root) {
        return Err(CompactRefuse::WindowsApps);
    }
    if let Some(fs) = volume_filesystem(root) {
        if fs.eq_ignore_ascii_case("ReFS") {
            return Err(CompactRefuse::ReFs);
        }
        if !fs.eq_ignore_ascii_case("NTFS") {
            return Err(CompactRefuse::NotNtfs { filesystem: fs });
        }
    }
    if !os_supports_wof() {
        return Err(CompactRefuse::UnsupportedOs);
    }
    if let Some(path) = running_exe_in_tree(root) {
        return Err(CompactRefuse::RunningExecutable { path });
    }
    if tree_contains_dstorage(root) && !allow_dstorage {
        return Err(CompactRefuse::DirectStorage {
            override_allowed: false,
        });
    }
    if let Some(app_id) = steam_updating_app_id(root) {
        return Err(CompactRefuse::SteamUpdating { app_id });
    }
    Ok(())
}

pub fn is_windows_apps_path(path: &Path) -> bool {
    path.to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
        .contains("\\windowsapps\\")
}

pub fn os_supports_wof() -> bool {
    #[cfg(windows)]
    {
        windows_build_number().map(|n| n >= 14393).unwrap_or(true)
    }
    #[cfg(not(windows))]
    {
        true
    }
}

#[cfg(windows)]
fn windows_build_number() -> Option<u32> {
    use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
    #[link(name = "ntdll")]
    extern "system" {
        fn RtlGetVersion(info: *mut OSVERSIONINFOW) -> i32;
    }
    unsafe {
        let mut info = OSVERSIONINFOW {
            dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
            ..Default::default()
        };
        if RtlGetVersion(&mut info) == 0 {
            Some(info.dwBuildNumber)
        } else {
            None
        }
    }
}

pub fn volume_filesystem(path: &Path) -> Option<String> {
    #[cfg(windows)]
    {
        windows_volume_filesystem(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Some("NTFS".into())
    }
}

#[cfg(windows)]
fn windows_volume_filesystem(path: &Path) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetVolumeInformationW;

    let root = volume_root(path)?;
    let wide: Vec<u16> = std::ffi::OsStr::new(&root)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut fs_name = [0u16; 64];
    let ok = unsafe {
        GetVolumeInformationW(
            PCWSTR(wide.as_ptr()),
            None,
            None,
            None,
            None,
            Some(&mut fs_name),
        )
    };
    if ok.is_err() {
        return None;
    }
    let end = fs_name
        .iter()
        .position(|c| *c == 0)
        .unwrap_or(fs_name.len());
    String::from_utf16(&fs_name[..end]).ok()
}

#[cfg(windows)]
fn volume_root(path: &Path) -> Option<String> {
    let s = path.to_string_lossy().replace('/', "\\");
    if s.len() >= 2 && s.as_bytes()[1] == b':' {
        Some(format!("{}\\", &s[..2]))
    } else if s.starts_with("\\\\") {
        Some(s)
    } else {
        None
    }
}

pub fn running_exe_in_tree(root: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        windows_running_exe_in_tree(root)
    }
    #[cfg(not(windows))]
    {
        let _ = root;
        None
    }
}

#[cfg(windows)]
fn windows_running_exe_in_tree(root: &Path) -> Option<PathBuf> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::ProcessStatus::{K32EnumProcesses, K32GetModuleFileNameExW};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    let root_key = normalize_for_cmp(root);
    let mut pids = vec![0u32; 1024];
    let mut needed = 0u32;
    let ok = unsafe { K32EnumProcesses(pids.as_mut_ptr(), (pids.len() * 4) as u32, &mut needed) };
    if !ok.as_bool() {
        return None;
    }
    let count = (needed as usize) / 4;
    for pid in pids.into_iter().take(count) {
        if pid == 0 {
            continue;
        }
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) };
        let Ok(handle) = handle else {
            continue;
        };
        let mut buf = [0u16; 520];
        let len = unsafe { K32GetModuleFileNameExW(Some(handle), None, &mut buf) };
        unsafe {
            let _ = CloseHandle(handle);
        }
        if len == 0 {
            continue;
        }
        let path = PathBuf::from(String::from_utf16_lossy(&buf[..len as usize]));
        if normalize_for_cmp(&path).starts_with(&root_key) {
            return Some(path);
        }
    }
    None
}

fn normalize_for_cmp(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

/// Apply compact or uncompact. Elevation is only requested after ACCESS_DENIED.
pub fn apply_compact(
    op: CompactOp,
    root: &Path,
    algorithm: CompactAlgorithm,
    allow_dstorage: bool,
    progress: impl FnMut(CompactProgress) + Send,
) -> Result<CompactResult, String> {
    super::apply::apply_wof(
        op,
        root,
        algorithm.for_live_library(),
        allow_dstorage,
        None,
        false,
        progress,
    )
}

/// Apply using the algorithm as given so Shelf can request LZX.
pub fn apply_compact_allowing_lzx(
    op: CompactOp,
    root: &Path,
    algorithm: CompactAlgorithm,
    allow_dstorage: bool,
    progress: impl FnMut(CompactProgress) + Send,
) -> Result<CompactResult, String> {
    super::apply::apply_wof(op, root, algorithm, allow_dstorage, None, false, progress)
}

/// User-initiated Change-method: rewrite already-compressed files (`/F`).
///
/// Incremental live compact must not use this. Maximum still keeps LZX.
pub fn apply_compact_force(
    op: CompactOp,
    root: &Path,
    algorithm: CompactAlgorithm,
    allow_dstorage: bool,
    progress: impl FnMut(CompactProgress) + Send,
) -> Result<CompactResult, String> {
    super::apply::apply_wof(op, root, algorithm, allow_dstorage, None, true, progress)
}

/// Incremental recompact of named files. Live algorithm only; never `/F` or root `/S`.
pub fn apply_incremental(
    root: &Path,
    files: &[PathBuf],
    algorithm: CompactAlgorithm,
    allow_dstorage: bool,
    progress: impl FnMut(CompactProgress) + Send,
) -> Result<CompactResult, String> {
    super::apply::apply_wof(
        CompactOp::Compress,
        root,
        algorithm.for_live_library(),
        allow_dstorage,
        Some(files),
        false,
        progress,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compact::build_compact_command;
    use crate::compact::command::{is_lznt1_command, is_wof_exe_command};
    use crate::compact::wof;

    #[test]
    fn windows_apps_paths_are_refused() {
        assert!(is_windows_apps_path(Path::new(
            r"C:\Program Files\WindowsApps\Foo.Game_1.0"
        )));
        assert!(!is_windows_apps_path(Path::new(
            r"D:\SteamLibrary\steamapps\common\Foo"
        )));
    }

    #[test]
    fn preflight_refuses_auto_excluded_title_folder() {
        let root = std::env::temp_dir().join(format!(
            "rusticgu-excl-{}-{}/Guild Wars 2",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let err = preflight(&root, false).unwrap_err();
        assert!(matches!(err, CompactRefuse::AutoExcluded { .. }));
        let _ = std::fs::remove_dir_all(root.parent().unwrap_or(&root));
    }

    #[test]
    fn apply_preflight_missing_folder() {
        let err = preflight(Path::new("/no/such/rusticgu/game"), false).unwrap_err();
        assert_eq!(err, CompactRefuse::MissingFolder);
    }

    #[test]
    fn estimate_skips_video_and_counts_rest() {
        let root = std::env::temp_dir().join(format!(
            "rusticgu-est-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(root.join("SaveGames")).unwrap();
        std::fs::write(root.join("game.exe"), vec![0u8; 1024]).unwrap();
        std::fs::write(root.join("movie.mp4"), vec![0u8; 4096]).unwrap();
        std::fs::write(root.join("SaveGames").join("slot.sav"), b"save").unwrap();
        let est = estimate_compact(&root, CompactAlgorithm::Xpress8k).unwrap();
        assert_eq!(est.file_count, 1);
        assert!(est.skipped_count >= 1);
        assert!(est.estimated_on_disk_bytes <= est.logical_bytes);
        let snap = measure_compact_sizes(&root);
        assert_eq!(snap.file_count, 1);
        assert_eq!(snap.logical_bytes, 1024);
        assert!(snap.on_disk_bytes > 0);
        assert_eq!(
            snap.saved_bytes(),
            snap.logical_bytes.saturating_sub(snap.on_disk_bytes)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn default_command_is_wof() {
        let inv = build_compact_command(
            CompactOp::Compress,
            Path::new(r"C:\g"),
            CompactAlgorithm::Xpress8k,
        );
        assert!(is_wof_exe_command(&inv));
        assert!(!is_lznt1_command(&inv));
        let line = inv.display_cmdline().to_ascii_uppercase();
        assert!(!line.contains("/S"), "{line}");
    }

    #[test]
    fn apply_uses_include_set_not_recursive_root() {
        use crate::compact::command::{
            apply_target_paths, build_apply_invocations, invocation_recurses_install_root,
        };

        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "rusticgu-apply-engine-{}-{}",
            std::process::id(),
            stamp
        ));
        std::fs::create_dir_all(root.join("SaveGames")).unwrap();
        std::fs::write(root.join("play.exe"), vec![0u8; 64]).unwrap();
        std::fs::write(root.join("cut.mp4"), vec![0u8; 64]).unwrap();
        std::fs::write(root.join("SaveGames").join("x.sav"), b"s").unwrap();

        let invs = build_apply_invocations(CompactOp::Compress, &root, CompactAlgorithm::Xpress8k);
        assert!(!invs.is_empty());
        for inv in &invs {
            assert!(!invocation_recurses_install_root(inv, &root));
            let line = inv.display_cmdline().to_ascii_uppercase();
            assert!(!line.contains("/S"), "{line}");
            assert!(!line.contains("CUT.MP4"), "{line}");
            assert!(!line.contains("SAVEGAMES"), "{line}");
            assert!(is_wof_exe_command(inv));
        }
        let targets = apply_target_paths(&root);
        assert_eq!(targets.len(), 1);
        assert_eq!(
            targets[0].file_name().and_then(|n| n.to_str()),
            Some("play.exe")
        );

        let result = apply_compact(
            CompactOp::Compress,
            &root,
            CompactAlgorithm::Xpress8k,
            false,
            |_| {},
        )
        .unwrap();
        assert!(result.ok);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn preflight_refuses_steam_updating_title() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let library = std::env::temp_dir().join(format!(
            "rusticgu-preflight-upd-{}-{}",
            std::process::id(),
            stamp
        ));
        let install = library.join("steamapps").join("common").join("BarGame");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join("game.exe"), b"exe").unwrap();
        std::fs::write(
            library.join("steamapps").join("appmanifest_99.acf"),
            r#"
"AppState"
{
	"appid"		"99"
	"name"		"Bar Game"
	"StateFlags"		"4"
	"installdir"		"BarGame"
}
"#,
        )
        .unwrap();
        assert!(preflight(&install, false).is_ok());

        std::fs::create_dir_all(library.join("steamapps").join("downloading").join("99")).unwrap();
        let err = preflight(&install, false).unwrap_err();
        assert_eq!(err, CompactRefuse::SteamUpdating { app_id: 99 });

        let _wof_stub = wof::test_reset();
        let apply_err = apply_compact(
            CompactOp::Compress,
            &install,
            CompactAlgorithm::Xpress8k,
            false,
            |_| {},
        )
        .unwrap_err();
        assert!(
            apply_err.contains("updating"),
            "apply must refuse before compact.exe: {apply_err}"
        );
        assert_eq!(
            wof::test_op_count(),
            0,
            "compact.exe must not spawn when Steam is updating"
        );

        let _ = std::fs::remove_dir_all(&library);
    }

    #[test]
    fn apply_compact_refuses_stateflags_updating_without_spawning() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let library = std::env::temp_dir().join(format!(
            "rusticgu-apply-flags-{}-{}",
            std::process::id(),
            stamp
        ));
        let install = library.join("steamapps").join("common").join("PatchGame");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join("game.exe"), b"exe").unwrap();
        std::fs::write(
            library.join("steamapps").join("appmanifest_1026.acf"),
            r#"
"AppState"
{
	"appid"		"1026"
	"name"		"Patch Game"
	"StateFlags"		"1026"
	"installdir"		"PatchGame"
}
"#,
        )
        .unwrap();
        assert_eq!(
            preflight(&install, false).unwrap_err(),
            CompactRefuse::SteamUpdating { app_id: 1026 }
        );
        let _wof_stub = wof::test_reset();
        assert!(apply_compact(
            CompactOp::Compress,
            &install,
            CompactAlgorithm::Xpress8k,
            false,
            |_| {},
        )
        .is_err());
        assert_eq!(wof::test_op_count(), 0);

        let _ = std::fs::remove_dir_all(&library);
    }

    #[test]
    fn maximum_lzx_file_failure_falls_back_and_still_finishes() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root =
            std::env::temp_dir().join(format!("rusticgu-lzx-fb-{}-{}", std::process::id(), stamp));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("play.exe"), vec![0u8; 64]).unwrap();
        std::fs::write(root.join("rusticgu-lzx-fail.dat"), vec![0u8; 64]).unwrap();

        let _wof_stub = wof::test_reset();
        let result = apply_compact_allowing_lzx(
            CompactOp::Compress,
            &root,
            CompactAlgorithm::Lzx,
            false,
            |_| {},
        )
        .expect("Maximum should finish via XPRESS16K fallback, not abort the title");
        assert!(result.ok);
        assert!(
            wof::test_op_count() >= 2,
            "batch fail must retry (got {})",
            wof::test_op_count()
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
