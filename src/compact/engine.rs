//! Compact / uncompact a game folder via WOF `compact /EXE`.

use std::path::{Path, PathBuf};

use crate::settings::CompactAlgorithm;

use super::command::{build_compact_command, CompactOp};
use super::skip::{should_skip, tree_contains_dstorage, walkdir_limited};

/// Typical XPRESS8K ratio used for dry-run estimates (conservative).
const XPRESS8K_RATIO: f64 = 0.62;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactRefuse {
    ReFs,
    WindowsApps,
    RunningExecutable { path: PathBuf },
    DirectStorage { override_allowed: bool },
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
                    write!(f, "dstorage.dll is present; override is enabled.")
                } else {
                    write!(
                        f,
                        "This install includes dstorage.dll (DirectStorage). Compact is blocked unless you enable the override in Settings → General."
                    )
                }
            }
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

/// Dry-run: walk the tree, apply the skip list, estimate savings.
pub fn estimate_compact(
    root: &Path,
    algorithm: CompactAlgorithm,
) -> Result<CompactEstimate, String> {
    if !root.is_dir() {
        return Err("Game folder is missing.".into());
    }
    let files = walkdir_limited(root, 24);
    let mut file_count = 0usize;
    let mut skipped_count = 0usize;
    let mut logical_bytes = 0u64;
    for path in &files {
        if should_skip(path) {
            skipped_count += 1;
            continue;
        }
        file_count += 1;
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

fn estimate_ratio(algorithm: CompactAlgorithm) -> f64 {
    match algorithm {
        CompactAlgorithm::Xpress4k => 0.70,
        CompactAlgorithm::Xpress8k => XPRESS8K_RATIO,
        CompactAlgorithm::Xpress16k => 0.55,
        CompactAlgorithm::Lzx => 0.48,
    }
}

/// Preflight guards. `allow_dstorage` lets the user override the dstorage.dll warning.
pub fn preflight(root: &Path, allow_dstorage: bool) -> Result<(), CompactRefuse> {
    if !root.exists() {
        return Err(CompactRefuse::MissingFolder);
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
        let len = unsafe { K32GetModuleFileNameExW(handle, None, &mut buf) };
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
    mut progress: impl FnMut(CompactProgress),
) -> Result<CompactResult, String> {
    preflight(root, allow_dstorage).map_err(|e| e.to_string())?;
    let estimate = estimate_compact(root, algorithm)?;
    progress(CompactProgress {
        processed: 0,
        total: estimate.file_count.max(1),
        message: match op {
            CompactOp::Compress => "Starting WOF compact…".into(),
            CompactOp::Uncompress => "Starting WOF uncompact…".into(),
        },
    });

    let inv = build_compact_command(op, root, algorithm);
    let output = run_compact(&inv, false)?;
    if is_access_denied(&output) {
        progress(CompactProgress {
            processed: 0,
            total: estimate.file_count.max(1),
            message: "Access denied — retrying elevated…".into(),
        });
        let output = run_compact(&inv, true)?;
        return interpret_output(op, output);
    }
    progress(CompactProgress {
        processed: estimate.file_count.max(1),
        total: estimate.file_count.max(1),
        message: "Finished.".into(),
    });
    interpret_output(op, output)
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

fn run_compact(
    inv: &super::command::CompactInvocation,
    elevate: bool,
) -> Result<CommandOutput, String> {
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
    }
}
