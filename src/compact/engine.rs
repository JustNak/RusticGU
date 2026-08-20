//! Compact / uncompact a game folder via WOF `compact /EXE`.

use std::path::{Path, PathBuf};

use crate::settings::CompactAlgorithm;

use crate::library::steam_updating_app_id;

use super::command::{
    build_apply_invocations_with_force, build_incremental_invocations, CompactOp,
};
use super::skip::{auto_excluded_title, collect_included_files, tree_contains_dstorage};

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
    let walked = super::skip::walkdir_limited(root, super::skip::COMPACT_WALK_DEPTH);
    let included = collect_included_files(root);
    let file_count = included.len();
    let skipped_count = walked.len().saturating_sub(file_count);
    let mut logical_bytes = 0u64;
    for path in &included {
        if let Ok(meta) = std::fs::metadata(path) {
            logical_bytes = logical_bytes.saturating_add(meta.len());
        }
    }
    let ratio = estimate_ratio(algorithm);
    let estimated_on_disk_bytes = (logical_bytes as f64 * ratio).round() as u64;
    Ok(CompactEstimate {
        file_count,
        skipped_count,
        logical_bytes,
        estimated_on_disk_bytes,
        has_dstorage: tree_contains_dstorage(root),
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
    let included = collect_included_files(root);
    let mut logical_bytes = 0u64;
    let mut on_disk_bytes = 0u64;
    for path in &included {
        let Ok(meta) = std::fs::metadata(path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let logical = meta.len();
        let on_disk = file_on_disk_bytes(path).unwrap_or(logical);
        logical_bytes = logical_bytes.saturating_add(logical);
        on_disk_bytes = on_disk_bytes.saturating_add(on_disk);
    }
    CompactSizeSnapshot {
        logical_bytes,
        on_disk_bytes,
        file_count: included.len(),
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
        // Unit tests / Linux CI: treat as supported so command construction can be tested.
        true
    }
}

#[cfg(windows)]
fn windows_build_number() -> Option<u32> {
    use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
    // RtlGetVersion is the supported way after GetVersionEx was deprecated.
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
    progress: impl FnMut(CompactProgress),
) -> Result<CompactResult, String> {
    apply_wof(
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
    progress: impl FnMut(CompactProgress),
) -> Result<CompactResult, String> {
    apply_wof(op, root, algorithm, allow_dstorage, None, false, progress)
}

/// User-initiated Change-method: rewrite already-compressed files (`/F`).
///
/// Incremental live compact must not use this. Maximum still keeps LZX.
pub fn apply_compact_force(
    op: CompactOp,
    root: &Path,
    algorithm: CompactAlgorithm,
    allow_dstorage: bool,
    progress: impl FnMut(CompactProgress),
) -> Result<CompactResult, String> {
    apply_wof(op, root, algorithm, allow_dstorage, None, true, progress)
}

/// Incremental recompact of named files. Live algorithm only; never `/F` or root `/S`.
pub fn apply_incremental(
    root: &Path,
    files: &[PathBuf],
    algorithm: CompactAlgorithm,
    allow_dstorage: bool,
    progress: impl FnMut(CompactProgress),
) -> Result<CompactResult, String> {
    apply_wof(
        CompactOp::Compress,
        root,
        algorithm.for_live_library(),
        allow_dstorage,
        Some(files),
        false,
        progress,
    )
}

fn apply_wof(
    op: CompactOp,
    root: &Path,
    algorithm: CompactAlgorithm,
    allow_dstorage: bool,
    explicit_files: Option<&[PathBuf]>,
    force: bool,
    mut progress: impl FnMut(CompactProgress),
) -> Result<CompactResult, String> {
    preflight(root, allow_dstorage).map_err(|e| e.to_string())?;
    let estimate = if explicit_files.is_some() {
        CompactEstimate {
            file_count: explicit_files.map(|f| f.len()).unwrap_or(1).max(1),
            skipped_count: 0,
            logical_bytes: 0,
            estimated_on_disk_bytes: 0,
            has_dstorage: super::skip::tree_contains_dstorage(root),
        }
    } else {
        estimate_compact_with(root, algorithm)?
    };
    progress(CompactProgress {
        processed: 0,
        total: estimate.file_count.max(1),
        message: match op {
            CompactOp::Compress => "Starting WOF compact…".into(),
            CompactOp::Uncompress => "Starting WOF uncompact…".into(),
        },
    });

    let invocations = match explicit_files {
        Some(files) => build_incremental_invocations(root, files, algorithm),
        None => {
            let coerce_live = algorithm.is_live();
            build_apply_invocations_with_force(op, root, algorithm, coerce_live, force)
        }
    };
    if invocations.is_empty() {
        return Ok(CompactResult {
            ok: true,
            message: "Nothing to compact (skip list excluded every file).".into(),
        });
    }

    let total = estimate.file_count.max(1);
    let mut elevate = false;
    let mut processed = 0usize;
    let mut last_output = CommandOutput {
        status_ok: true,
        stdout: String::new(),
        stderr: String::new(),
        code: Some(0),
    };

    for inv in &invocations {
        let file_n = inv
            .args
            .iter()
            .filter(|a| !a.to_string_lossy().starts_with('/'))
            .count();
        let mut output = run_compact(inv, elevate)?;
        if is_access_denied(&output) && !elevate {
            progress(CompactProgress {
                processed,
                total,
                message: "Access denied. Retrying elevated…".into(),
            });
            elevate = true;
            output = run_compact(inv, true)?;
        }
        if !output.status_ok && !is_access_denied(&output) {
            return interpret_output(op, output);
        }
        processed = processed.saturating_add(file_n);
        progress(CompactProgress {
            processed: processed.min(total),
            total,
            message: format!("WOF /EXE {processed}/{total}…"),
        });
        last_output = output;
    }

    progress(CompactProgress {
        processed: total,
        total,
        message: "Finished.".into(),
    });
    interpret_output(op, last_output)
}

struct CommandOutput {
    status_ok: bool,
    stdout: String,
    stderr: String,
    code: Option<i32>,
}

fn is_access_denied(output: &CommandOutput) -> bool {
    let blob = format!("{}\n{}", output.stdout, output.stderr).to_ascii_lowercase();
    output.code == Some(5) || blob.contains("access is denied") || blob.contains("access denied")
}

fn interpret_output(op: CompactOp, output: CommandOutput) -> Result<CompactResult, String> {
    if output.status_ok {
        let verb = match op {
            CompactOp::Compress => "Compacted",
            CompactOp::Uncompress => "Uncompacted",
        };
        Ok(CompactResult {
            ok: true,
            message: format!("{verb} with WOF /EXE."),
        })
    } else {
        let detail = if output.stderr.trim().is_empty() {
            output.stdout.trim().to_string()
        } else {
            output.stderr.trim().to_string()
        };
        Err(if detail.is_empty() {
            format!("compact.exe failed (exit {}).", output.code.unwrap_or(-1))
        } else {
            detail
        })
    }
}

#[cfg(test)]
thread_local! {
    static COMPACT_SPAWN_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn run_compact(
    inv: &super::command::CompactInvocation,
    elevate: bool,
) -> Result<CommandOutput, String> {
    #[cfg(test)]
    COMPACT_SPAWN_COUNT.with(|c| c.set(c.get().saturating_add(1)));
    #[cfg(windows)]
    {
        windows_run(inv, elevate)
    }
    #[cfg(not(windows))]
    {
        let _ = elevate;
        Ok(CommandOutput {
            status_ok: true,
            stdout: format!("dry {}", inv.display_cmdline()),
            stderr: String::new(),
            code: Some(0),
        })
    }
}

#[cfg(windows)]
fn windows_run(
    inv: &super::command::CompactInvocation,
    elevate: bool,
) -> Result<CommandOutput, String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    if elevate {
        return windows_run_elevated(inv);
    }

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = Command::new(&inv.program)
        .args(&inv.args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Could not start compact.exe: {e}"))?;
    Ok(CommandOutput {
        status_ok: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code(),
    })
}

#[cfg(windows)]
fn windows_run_elevated(inv: &super::command::CompactInvocation) -> Result<CommandOutput, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::WaitForSingleObject;
    use windows::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    let file = wide(std::ffi::OsStr::new("compact.exe"));
    let verb = wide(std::ffi::OsStr::new("runas"));
    let mut params = String::new();
    for (i, arg) in inv.args.iter().enumerate() {
        if i > 0 {
            params.push(' ');
        }
        let s = arg.to_string_lossy();
        if s.chars().any(|c| c.is_whitespace()) {
            params.push('"');
            params.push_str(&s);
            params.push('"');
        } else {
            params.push_str(&s);
        }
    }
    let params_w = wide(std::ffi::OsStr::new(&params));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params_w.as_ptr()),
        nShow: SW_HIDE.0 as i32,
        ..Default::default()
    };
    let ok = unsafe { ShellExecuteExW(&mut info) };
    if ok.is_err() {
        return Err("Could not elevate compact.exe (UAC cancelled or failed).".into());
    }
    if !info.hProcess.is_invalid() {
        unsafe {
            let _ = WaitForSingleObject(info.hProcess, 30 * 60 * 1000);
            let _ = CloseHandle(info.hProcess);
        }
    }
    Ok(CommandOutput {
        status_ok: true,
        stdout: String::new(),
        stderr: String::new(),
        code: Some(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compact::build_compact_command;
    use crate::compact::command::{is_lznt1_command, is_wof_exe_command};

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

        COMPACT_SPAWN_COUNT.with(|c| c.set(0));
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
            COMPACT_SPAWN_COUNT.with(|c| c.get()),
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
        COMPACT_SPAWN_COUNT.with(|c| c.set(0));
        assert!(apply_compact(
            CompactOp::Compress,
            &install,
            CompactAlgorithm::Xpress8k,
            false,
            |_| {},
        )
        .is_err());
        assert_eq!(COMPACT_SPAWN_COUNT.with(|c| c.get()), 0);

        let _ = std::fs::remove_dir_all(&library);
    }
}
