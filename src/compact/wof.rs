//! Native WOF CompactOS (`WOF_PROVIDER_FILE`) compress / uncompress.
//!
//! Production Windows uses `WofUtil.dll` + `FSCTL_DELETE_EXTERNAL_BACKING`.
//! Unit tests (including Windows CI) use an in-process stub so results are
//! deterministic and do not require UAC or a real NTFS WOF driver.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::settings::CompactAlgorithm;

use super::command::CompactOp;

#[cfg_attr(test, allow(dead_code))]
const WOF_PROVIDER_FILE: u32 = 2;
const FILE_PROVIDER_COMPRESSION_XPRESS4K: u32 = 0;
const FILE_PROVIDER_COMPRESSION_LZX: u32 = 1;
const FILE_PROVIDER_COMPRESSION_XPRESS8K: u32 = 2;
const FILE_PROVIDER_COMPRESSION_XPRESS16K: u32 = 3;

const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_SHARING_VIOLATION: u32 = 32;
const ERROR_LOCK_VIOLATION: u32 = 33;
const ERROR_COMPRESSION_NOT_BENEFICIAL: u32 = 344;

#[cfg_attr(test, allow(dead_code))]
const FSCTL_DELETE_EXTERNAL_BACKING: u32 = 0x0009_0314;

const LZX_PROBE_BYTES: u64 = 16 * 1024 * 1024;
const XPRESS_PROBE_BYTES: u64 = 64 * 1024 * 1024;
const PROBE_SAMPLE: usize = 64 * 1024;
const PROBE_COUNT: usize = 6;
const INCOMPRESSIBLE_RATIO: f64 = 0.95;

#[cfg_attr(test, allow(dead_code))]
#[repr(C)]
struct WofFileCompressionInfoV1 {
    algorithm: u32,
    flags: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WofStatus {
    Applied,
    NotBeneficial,
    AlreadySame,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WofError {
    AccessDenied,
    SharingViolation,
    Failed(String),
}

impl std::fmt::Display for WofError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AccessDenied => write!(f, "Access is denied."),
            Self::SharingViolation => write!(f, "File is in use."),
            Self::Failed(msg) => write!(f, "{msg}"),
        }
    }
}

pub fn algorithm_provider_id(algorithm: CompactAlgorithm) -> u32 {
    match algorithm {
        CompactAlgorithm::Xpress4k | CompactAlgorithm::Xpress => FILE_PROVIDER_COMPRESSION_XPRESS4K,
        CompactAlgorithm::Lzx => FILE_PROVIDER_COMPRESSION_LZX,
        CompactAlgorithm::Xpress8k => FILE_PROVIDER_COMPRESSION_XPRESS8K,
        CompactAlgorithm::Xpress16k => FILE_PROVIDER_COMPRESSION_XPRESS16K,
    }
}

pub fn algorithm_from_provider_id(id: u32) -> Option<CompactAlgorithm> {
    match id {
        FILE_PROVIDER_COMPRESSION_XPRESS4K => Some(CompactAlgorithm::Xpress4k),
        FILE_PROVIDER_COMPRESSION_LZX => Some(CompactAlgorithm::Lzx),
        FILE_PROVIDER_COMPRESSION_XPRESS8K => Some(CompactAlgorithm::Xpress8k),
        FILE_PROVIDER_COMPRESSION_XPRESS16K => Some(CompactAlgorithm::Xpress16k),
        _ => None,
    }
}

pub fn same_wof_algorithm(existing: CompactAlgorithm, requested: CompactAlgorithm) -> bool {
    algorithm_provider_id(existing) == algorithm_provider_id(requested)
}

/// Skip files smaller than one NTFS cluster. `cluster <= 1` disables the skip
/// (used by unit tests so tiny fixtures still exercise the apply path).
pub fn too_small(len: u64, cluster: u64) -> bool {
    cluster > 1 && len < cluster
}

pub fn worker_count(algorithm: CompactAlgorithm, seek_penalty: Option<bool>) -> usize {
    let n = std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(4)
        .max(1);
    if algorithm == CompactAlgorithm::Lzx {
        return (n / 2).clamp(1, 4);
    }
    match seek_penalty {
        Some(true) => 2,
        Some(false) => n.min(12).max(1),
        None => n.min(4).max(1),
    }
}

pub fn effective_cluster(root: &Path) -> u64 {
    if let Some(c) = test_cluster() {
        return c;
    }
    #[cfg(test)]
    {
        let _ = root;
        return 1;
    }
    #[cfg(all(windows, not(test)))]
    {
        return volume_cluster_size(root).unwrap_or(4096);
    }
    #[cfg(all(not(windows), not(test)))]
    {
        let _ = root;
        4096
    }
}

pub fn volume_seek_penalty(root: &Path) -> Option<bool> {
    if let Some(p) = test_seek_penalty() {
        return Some(p);
    }
    #[cfg(all(windows, not(test)))]
    {
        return windows_seek_penalty(root);
    }
    #[cfg(not(all(windows, not(test))))]
    {
        let _ = root;
        None
    }
}

pub fn wof_runtime_available() -> bool {
    #[cfg(test)]
    {
        return true;
    }
    #[cfg(all(windows, not(test)))]
    {
        return wofutil_present();
    }
    #[cfg(all(not(windows), not(test)))]
    {
        true
    }
}

pub fn detect(path: &Path) -> Result<Option<CompactAlgorithm>, WofError> {
    #[cfg(test)]
    {
        return Ok(test_backing(path));
    }
    #[cfg(all(windows, not(test)))]
    {
        return windows_detect(path);
    }
    #[cfg(all(not(windows), not(test)))]
    {
        let _ = path;
        Ok(None)
    }
}

pub fn compress_file(path: &Path, algorithm: CompactAlgorithm) -> Result<WofStatus, WofError> {
    record_op();
    #[cfg(test)]
    {
        return test_compress(path, algorithm);
    }
    #[cfg(all(windows, not(test)))]
    {
        return windows_compress(path, algorithm);
    }
    #[cfg(all(not(windows), not(test)))]
    {
        let _ = (path, algorithm);
        Ok(WofStatus::Applied)
    }
}

pub fn uncompress_file(path: &Path) -> Result<WofStatus, WofError> {
    record_op();
    #[cfg(test)]
    {
        return test_uncompress(path);
    }
    #[cfg(all(windows, not(test)))]
    {
        return windows_uncompress(path);
    }
    #[cfg(all(not(windows), not(test)))]
    {
        let _ = path;
        Ok(WofStatus::Applied)
    }
}

pub fn looks_incompressible(path: &Path, algorithm: CompactAlgorithm, op: CompactOp) -> bool {
    if op != CompactOp::Compress {
        return false;
    }
    if test_is_incompressible(path) {
        return true;
    }
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    let threshold = if algorithm == CompactAlgorithm::Lzx {
        LZX_PROBE_BYTES
    } else {
        XPRESS_PROBE_BYTES
    };
    if meta.len() < threshold {
        return false;
    }
    match sample_lz4_ratio(path) {
        Some(ratio) => ratio >= INCOMPRESSIBLE_RATIO,
        None => false,
    }
}

pub fn sample_lz4_ratio(path: &Path) -> Option<f64> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len < 256 {
        return None;
    }
    let mut ratios = Vec::new();
    for i in 0..PROBE_COUNT {
        let pos = (i as u64).saturating_mul(len) / PROBE_COUNT as u64;
        file.seek(SeekFrom::Start(pos)).ok()?;
        let take = PROBE_SAMPLE.min(len.saturating_sub(pos) as usize);
        if take < 256 {
            continue;
        }
        let mut buf = vec![0u8; take];
        let n = file.read(&mut buf).ok()?;
        if n < 256 {
            continue;
        }
        buf.truncate(n);
        ratios.push(lz4_ratio(&buf));
    }
    if ratios.is_empty() {
        None
    } else {
        Some(ratios.iter().sum::<f64>() / ratios.len() as f64)
    }
}

pub fn lz4_ratio(bytes: &[u8]) -> f64 {
    if bytes.is_empty() {
        return 1.0;
    }
    let compressed = lz4_flex::compress(bytes);
    compressed.len() as f64 / bytes.len() as f64
}

pub fn collect_wof_backed_files(root: &Path) -> Vec<std::path::PathBuf> {
    super::skip::walk_all_files(root, super::skip::COMPACT_WALK_DEPTH)
        .into_iter()
        .filter(|p| detect(p).ok().flatten().is_some())
        .collect()
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

fn hresult_win32(hr: i32) -> u32 {
    if hr >= 0 {
        hr as u32
    } else {
        (hr as u32) & 0xFFFF
    }
}

#[cfg_attr(test, allow(dead_code))]
fn map_win32(code: u32, ctx: &str) -> WofError {
    match code {
        ERROR_ACCESS_DENIED => WofError::AccessDenied,
        ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION => WofError::SharingViolation,
        ERROR_COMPRESSION_NOT_BENEFICIAL => {
            WofError::Failed("internal: not beneficial mapped as error".into())
        }
        _ => WofError::Failed(format!("{ctx} (win32 {code})")),
    }
}

#[cfg(all(windows, not(test)))]
fn wofutil_present() -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::FreeLibrary;
    use windows::Win32::System::LibraryLoader::LoadLibraryW;

    let wide: Vec<u16> = std::ffi::OsStr::new("WofUtil.dll")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let handle = unsafe { LoadLibraryW(PCWSTR(wide.as_ptr())) };
    match handle {
        Ok(h) if !h.is_invalid() => {
            let _ = unsafe { FreeLibrary(h) };
            true
        }
        _ => false,
    }
}

#[cfg(all(windows, not(test)))]
fn wof_proc(name: &std::ffi::CStr) -> windows::Win32::Foundation::FARPROC {
    use std::os::windows::ffi::OsStrExt;
    use std::sync::OnceLock;
    use windows::core::{PCSTR, PCWSTR};
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    // HMODULE is a raw pointer and is not Send/Sync, so cache the address as isize.
    static MODULE: OnceLock<isize> = OnceLock::new();
    let raw = *MODULE.get_or_init(|| {
        let wide: Vec<u16> = std::ffi::OsStr::new("WofUtil.dll")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        match unsafe { LoadLibraryW(PCWSTR(wide.as_ptr())) } {
            Ok(h) if !h.is_invalid() => h.0 as isize,
            _ => 0,
        }
    });
    if raw == 0 {
        return None;
    }
    let module = HMODULE(raw as *mut core::ffi::c_void);
    unsafe { GetProcAddress(module, PCSTR(name.as_ptr().cast())) }
}

#[cfg(all(windows, not(test)))]
fn windows_detect(path: &Path) -> Result<Option<CompactAlgorithm>, WofError> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;

    type WofIsExternalFileFn = unsafe extern "system" fn(
        file_path: PCWSTR,
        is_external_file: *mut i32,
        provider: *mut u32,
        external_file_info: *mut core::ffi::c_void,
        length: *mut u32,
    ) -> i32;

    let Some(proc) = wof_proc(c"WofIsExternalFile") else {
        return Ok(None);
    };
    let func: WofIsExternalFileFn = unsafe { std::mem::transmute(proc) };
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut is_external = 0i32;
    let mut provider = 0u32;
    let mut info = WofFileCompressionInfoV1 {
        algorithm: 0,
        flags: 0,
    };
    let mut len = std::mem::size_of::<WofFileCompressionInfoV1>() as u32;
    let hr = unsafe {
        func(
            PCWSTR(wide.as_ptr()),
            &mut is_external,
            &mut provider,
            (&mut info as *mut WofFileCompressionInfoV1).cast(),
            &mut len,
        )
    };
    if hr < 0 {
        let code = hresult_win32(hr);
        if code == 0 {
            return Ok(None);
        }
        return Err(map_win32(code, "WofIsExternalFile"));
    }
    if is_external == 0 || provider != WOF_PROVIDER_FILE {
        return Ok(None);
    }
    Ok(algorithm_from_provider_id(info.algorithm))
}

#[cfg(all(windows, not(test)))]
struct WofFile {
    handle: windows::Win32::Foundation::HANDLE,
    wide: Vec<u16>,
    restore_readonly: bool,
}

#[cfg(all(windows, not(test)))]
impl Drop for WofFile {
    fn drop(&mut self) {
        use windows::Win32::Foundation::CloseHandle;
        unsafe {
            let _ = CloseHandle(self.handle);
        }
        if self.restore_readonly {
            restore_readonly_attribute(&self.wide);
        }
    }
}

#[cfg(all(windows, not(test)))]
fn open_wof_file(path: &Path) -> Result<WofFile, WofError> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING,
    };

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let restore_readonly = clear_readonly_attribute(&wide);
    match unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            (GENERIC_READ.0 | GENERIC_WRITE.0) as u32,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    } {
        Ok(handle) => Ok(WofFile {
            handle,
            wide,
            restore_readonly,
        }),
        Err(e) => {
            if restore_readonly {
                restore_readonly_attribute(&wide);
            }
            Err(map_win32(hresult_win32(e.code().0), "CreateFileW"))
        }
    }
}

#[cfg(all(windows, not(test)))]
fn clear_readonly_attribute(wide: &[u16]) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        GetFileAttributesW, SetFileAttributesW, FILE_ATTRIBUTE_READONLY, FILE_FLAGS_AND_ATTRIBUTES,
        INVALID_FILE_ATTRIBUTES,
    };

    let attrs = unsafe { GetFileAttributesW(PCWSTR(wide.as_ptr())) };
    if attrs == INVALID_FILE_ATTRIBUTES {
        return false;
    }
    let readonly = FILE_ATTRIBUTE_READONLY.0;
    if attrs & readonly == 0 {
        return false;
    }
    unsafe {
        SetFileAttributesW(
            PCWSTR(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(attrs & !readonly),
        )
    }
    .is_ok()
}

#[cfg(all(windows, not(test)))]
fn restore_readonly_attribute(wide: &[u16]) {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        GetFileAttributesW, SetFileAttributesW, FILE_ATTRIBUTE_READONLY, FILE_FLAGS_AND_ATTRIBUTES,
        INVALID_FILE_ATTRIBUTES,
    };

    let attrs = unsafe { GetFileAttributesW(PCWSTR(wide.as_ptr())) };
    if attrs == INVALID_FILE_ATTRIBUTES {
        return;
    }
    let readonly = FILE_ATTRIBUTE_READONLY.0;
    if attrs & readonly != 0 {
        return;
    }
    let _ = unsafe {
        SetFileAttributesW(
            PCWSTR(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(attrs | readonly),
        )
    };
}

#[cfg(all(windows, not(test)))]
fn windows_compress(path: &Path, algorithm: CompactAlgorithm) -> Result<WofStatus, WofError> {
    type WofSetFn = unsafe extern "system" fn(
        windows::Win32::Foundation::HANDLE,
        u32,
        *const core::ffi::c_void,
        u32,
    ) -> i32;

    let Some(proc) = wof_proc(c"WofSetFileDataLocation") else {
        return Err(WofError::Failed("WofUtil.dll is unavailable.".into()));
    };
    let func: WofSetFn = unsafe { std::mem::transmute(proc) };
    let file = open_wof_file(path)?;
    let info = WofFileCompressionInfoV1 {
        algorithm: algorithm_provider_id(algorithm),
        flags: 0,
    };
    let hr = unsafe {
        func(
            file.handle,
            WOF_PROVIDER_FILE,
            (&info as *const WofFileCompressionInfoV1).cast(),
            std::mem::size_of::<WofFileCompressionInfoV1>() as u32,
        )
    };
    drop(file);
    if hr >= 0 {
        return Ok(WofStatus::Applied);
    }
    let code = hresult_win32(hr);
    if code == ERROR_COMPRESSION_NOT_BENEFICIAL {
        return Ok(WofStatus::NotBeneficial);
    }
    Err(map_win32(code, "WofSetFileDataLocation"))
}

#[cfg(all(windows, not(test)))]
fn windows_uncompress(path: &Path) -> Result<WofStatus, WofError> {
    use windows::Win32::System::IO::DeviceIoControl;

    let file = open_wof_file(path)?;
    let mut returned = 0u32;
    let result = unsafe {
        DeviceIoControl(
            file.handle,
            FSCTL_DELETE_EXTERNAL_BACKING,
            None,
            0,
            None,
            0,
            Some(&mut returned as *mut u32),
            None,
        )
    };
    drop(file);
    match result {
        Ok(()) => Ok(WofStatus::Applied),
        Err(e) => {
            let code = hresult_win32(e.code().0);
            if code == 0 || code == 4390 {
                // STATUS_OBJECT_NOT_EXTERNALLY_BACKED-ish / not backed.
                return Ok(WofStatus::AlreadySame);
            }
            Err(map_win32(code, "FSCTL_DELETE_EXTERNAL_BACKING"))
        }
    }
}

#[cfg(all(windows, not(test)))]
fn volume_cluster_size(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceW;

    let root = volume_root(path)?;
    let wide: Vec<u16> = std::ffi::OsStr::new(&root)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut sectors_per_cluster = 0u32;
    let mut bytes_per_sector = 0u32;
    let mut free_clusters = 0u32;
    let mut total_clusters = 0u32;
    let ok = unsafe {
        GetDiskFreeSpaceW(
            PCWSTR(wide.as_ptr()),
            Some(&mut sectors_per_cluster),
            Some(&mut bytes_per_sector),
            Some(&mut free_clusters),
            Some(&mut total_clusters),
        )
    };
    if ok.is_err() {
        return None;
    }
    Some(u64::from(sectors_per_cluster) * u64::from(bytes_per_sector))
}

#[cfg(all(windows, not(test)))]
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

#[cfg(all(windows, not(test)))]
fn windows_seek_penalty(path: &Path) -> Option<bool> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, GENERIC_READ};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::Ioctl::{
        PropertyStandardQuery, StorageDeviceSeekPenaltyProperty, DEVICE_SEEK_PENALTY_DESCRIPTOR,
        IOCTL_STORAGE_QUERY_PROPERTY, STORAGE_PROPERTY_QUERY,
    };
    use windows::Win32::System::IO::DeviceIoControl;

    let root = volume_root(path)?;
    let drive = root.chars().next()?.to_ascii_uppercase();
    if !drive.is_ascii_alphabetic() {
        return None;
    }
    let device = format!(r"\\.\{drive}:");
    let wide: Vec<u16> = std::ffi::OsStr::new(&device)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }
    .ok()?;
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceSeekPenaltyProperty,
        QueryType: PropertyStandardQuery,
        AdditionalParameters: [0; 1],
    };
    let mut desc = DEVICE_SEEK_PENALTY_DESCRIPTOR::default();
    let mut returned = 0u32;
    let ok = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some((&query as *const STORAGE_PROPERTY_QUERY).cast()),
            std::mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            Some((&mut desc as *mut DEVICE_SEEK_PENALTY_DESCRIPTOR).cast()),
            std::mem::size_of::<DEVICE_SEEK_PENALTY_DESCRIPTOR>() as u32,
            Some(&mut returned as *mut u32),
            None,
        )
    };
    unsafe {
        let _ = CloseHandle(handle);
    }
    if ok.is_err() {
        return None;
    }
    Some(desc.IncursSeekPenalty)
}

#[cfg(test)]
#[derive(Default)]
struct TestState {
    ops: usize,
    backing: std::collections::HashMap<String, CompactAlgorithm>,
    access_denied: std::collections::HashSet<String>,
    always_denied: std::collections::HashSet<String>,
    sharing: std::collections::HashSet<String>,
    not_beneficial: std::collections::HashSet<String>,
    incompressible: std::collections::HashSet<String>,
    hard_fail: std::collections::HashSet<String>,
    cluster: Option<u64>,
    seek_penalty: Option<bool>,
    elevated: bool,
}

/// Held for the lifetime of a WOF stub test so parallel `cargo test` threads
/// cannot clobber shared stub state. Worker threads still share `TEST_STATE`.
#[cfg(test)]
#[must_use = "keep this guard alive for the whole test"]
pub struct WofStubGuard {
    _serial: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
static TEST_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
static TEST_STATE: std::sync::LazyLock<std::sync::Mutex<TestState>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(TestState::default()));

#[cfg(test)]
fn lock_test() -> std::sync::MutexGuard<'static, TestState> {
    TEST_STATE.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
fn with_test<R>(f: impl FnOnce(&mut TestState) -> R) -> R {
    let mut state = lock_test();
    f(&mut state)
}

fn record_op() {
    #[cfg(test)]
    {
        with_test(|state| {
            state.ops = state.ops.saturating_add(1);
        });
    }
}

fn test_cluster() -> Option<u64> {
    #[cfg(test)]
    {
        return with_test(|state| state.cluster);
    }
    #[cfg(not(test))]
    None
}

fn test_seek_penalty() -> Option<bool> {
    #[cfg(test)]
    {
        return with_test(|state| state.seek_penalty);
    }
    #[cfg(not(test))]
    None
}

fn test_is_incompressible(path: &Path) -> bool {
    #[cfg(test)]
    {
        return with_test(|state| state.incompressible.contains(&path_key(path)));
    }
    #[cfg(not(test))]
    {
        let _ = path;
        false
    }
}

#[cfg(test)]
fn test_backing(path: &Path) -> Option<CompactAlgorithm> {
    with_test(|state| state.backing.get(&path_key(path)).copied())
}

#[cfg(test)]
fn test_compress(path: &Path, algorithm: CompactAlgorithm) -> Result<WofStatus, WofError> {
    let key = path_key(path);
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    with_test(|state| {
        if name.contains("rusticgu-lzx-fail") && algorithm == CompactAlgorithm::Lzx {
            return Err(WofError::Failed("simulated LZX failure".into()));
        }
        if state.hard_fail.contains(&key) {
            return Err(WofError::Failed("simulated WOF failure".into()));
        }
        if state.sharing.contains(&key) {
            return Err(WofError::SharingViolation);
        }
        if state.always_denied.contains(&key) {
            return Err(WofError::AccessDenied);
        }
        if state.access_denied.contains(&key) && !state.elevated {
            return Err(WofError::AccessDenied);
        }
        if state.not_beneficial.contains(&key) {
            return Ok(WofStatus::NotBeneficial);
        }
        state.backing.insert(key, algorithm);
        Ok(WofStatus::Applied)
    })
}

#[cfg(test)]
fn test_uncompress(path: &Path) -> Result<WofStatus, WofError> {
    let key = path_key(path);
    with_test(|state| {
        if state.sharing.contains(&key) {
            return Err(WofError::SharingViolation);
        }
        if state.always_denied.contains(&key) {
            return Err(WofError::AccessDenied);
        }
        if state.access_denied.contains(&key) && !state.elevated {
            return Err(WofError::AccessDenied);
        }
        if state.backing.remove(&key).is_none() {
            return Ok(WofStatus::AlreadySame);
        }
        Ok(WofStatus::Applied)
    })
}

#[cfg(test)]
pub fn test_reset() -> WofStubGuard {
    let serial = TEST_SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    *lock_test() = TestState::default();
    WofStubGuard { _serial: serial }
}

#[cfg(test)]
pub fn test_op_count() -> usize {
    with_test(|state| state.ops)
}

#[cfg(test)]
pub fn test_set_ops(n: usize) {
    with_test(|state| state.ops = n);
}

#[cfg(test)]
pub fn test_set_backing(path: &Path, algorithm: Option<CompactAlgorithm>) {
    let key = path_key(path);
    with_test(|state| match algorithm {
        Some(a) => {
            state.backing.insert(key, a);
        }
        None => {
            state.backing.remove(&key);
        }
    });
}

#[cfg(test)]
pub fn test_set_access_denied(path: &Path, denied: bool) {
    let key = path_key(path);
    with_test(|state| {
        if denied {
            state.access_denied.insert(key);
        } else {
            state.access_denied.remove(&key);
        }
    });
}

#[cfg(test)]
pub fn test_set_always_denied(path: &Path, denied: bool) {
    let key = path_key(path);
    with_test(|state| {
        if denied {
            state.always_denied.insert(key);
        } else {
            state.always_denied.remove(&key);
        }
    });
}

#[cfg(test)]
pub fn test_set_not_beneficial(path: &Path, value: bool) {
    let key = path_key(path);
    with_test(|state| {
        if value {
            state.not_beneficial.insert(key);
        } else {
            state.not_beneficial.remove(&key);
        }
    });
}

#[cfg(test)]
pub fn test_set_incompressible(path: &Path, value: bool) {
    let key = path_key(path);
    with_test(|state| {
        if value {
            state.incompressible.insert(key);
        } else {
            state.incompressible.remove(&key);
        }
    });
}

#[cfg(test)]
pub fn test_set_hard_fail(path: &Path, value: bool) {
    let key = path_key(path);
    with_test(|state| {
        if value {
            state.hard_fail.insert(key);
        } else {
            state.hard_fail.remove(&key);
        }
    });
}

#[cfg(test)]
pub fn test_set_sharing(path: &Path, value: bool) {
    let key = path_key(path);
    with_test(|state| {
        if value {
            state.sharing.insert(key);
        } else {
            state.sharing.remove(&key);
        }
    });
}

#[cfg(test)]
pub fn test_set_cluster(cluster: Option<u64>) {
    with_test(|state| state.cluster = cluster);
}

#[cfg(test)]
pub fn test_set_elevated(elevated: bool) {
    with_test(|state| state.elevated = elevated);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_ids_match_wof_file_provider() {
        assert_eq!(algorithm_provider_id(CompactAlgorithm::Xpress4k), 0);
        assert_eq!(algorithm_provider_id(CompactAlgorithm::Lzx), 1);
        assert_eq!(algorithm_provider_id(CompactAlgorithm::Xpress8k), 2);
        assert_eq!(algorithm_provider_id(CompactAlgorithm::Xpress16k), 3);
        assert_eq!(algorithm_provider_id(CompactAlgorithm::Xpress), 0);
        assert_eq!(
            algorithm_from_provider_id(FILE_PROVIDER_COMPRESSION_XPRESS8K),
            Some(CompactAlgorithm::Xpress8k)
        );
        assert_eq!(algorithm_from_provider_id(99), None);
        assert!(same_wof_algorithm(
            CompactAlgorithm::Xpress,
            CompactAlgorithm::Xpress4k
        ));
        assert!(!same_wof_algorithm(
            CompactAlgorithm::Xpress8k,
            CompactAlgorithm::Xpress16k
        ));
    }

    #[test]
    fn tiny_files_skip_when_cluster_known() {
        assert!(!too_small(100, 1));
        assert!(too_small(100, 4096));
        assert!(!too_small(8192, 4096));
    }

    #[test]
    fn worker_caps_lzx_and_hdd() {
        let n = worker_count(CompactAlgorithm::Lzx, Some(false));
        assert!((1..=4).contains(&n), "{n}");
        assert_eq!(worker_count(CompactAlgorithm::Xpress8k, Some(true)), 2);
        let ssd = worker_count(CompactAlgorithm::Xpress8k, Some(false));
        assert!((1..=12).contains(&ssd));
    }

    #[test]
    fn lz4_ratio_zeros_beats_noise() {
        let zeros = vec![0u8; 4096];
        let mut noise = vec![0u8; 4096];
        for (i, b) in noise.iter_mut().enumerate() {
            *b = (i.wrapping_mul(197) ^ 0xA5) as u8;
        }
        assert!(lz4_ratio(&zeros) < 0.2, "{}", lz4_ratio(&zeros));
        assert!(lz4_ratio(&noise) > lz4_ratio(&zeros));
    }

    #[test]
    fn hresult_extracts_win32_code() {
        assert_eq!(hresult_win32(0), 0);
        assert_eq!(hresult_win32(344), 344);
        assert_eq!(hresult_win32(-2147024552), ERROR_COMPRESSION_NOT_BENEFICIAL); // 0x80070158
        assert_eq!(hresult_win32(-2147024891), ERROR_ACCESS_DENIED); // 0x80070005
        assert!(matches!(
            map_win32(ERROR_SHARING_VIOLATION, "open"),
            WofError::SharingViolation
        ));
        assert!(matches!(
            map_win32(ERROR_LOCK_VIOLATION, "open"),
            WofError::SharingViolation
        ));
        assert!(matches!(
            map_win32(ERROR_ACCESS_DENIED, "open"),
            WofError::AccessDenied
        ));
    }
}
