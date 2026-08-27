//! Live Compact: drive `crates/watch` and apply incremental WOF on the include-set.
//!
//! This is the app-side machine. Do not reimplement watch internals here.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use watch::{
    title_from_acf_text, Compactor, FileCompactState, FileInventory, IncrementalPlan, InstallFile,
    LiveWatch, SteamStatus, TickEvent, TitleStatus, WatchResult,
};

use crate::compact::{
    apply_incremental, build_incremental_invocations, collect_included_files,
    invocation_has_force_flag, invocation_recurses_install_root, path_is_auto_excluded,
    should_skip,
};
use crate::library::{
    collect_library_folders, downloading_folder_present, steam_path, LibraryTitle,
};
use crate::settings::CompactAlgorithm;
use shelf::default_denylist;

#[derive(Debug, Clone)]
pub struct StoredPlan {
    pub plan: IncrementalPlan,
    pub install: PathBuf,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LockEntry {
    compact: FileCompactState,
    len: u64,
}

pub struct LiveInner {
    pub paused: AtomicBool,
    pub compact_busy: AtomicBool,
    pub allow_dstorage: AtomicBool,
    pub last_plan: Mutex<Option<StoredPlan>>,
    pub locked: Mutex<BTreeSet<String>>,
    pub installs: Mutex<HashMap<String, PathBuf>>,
    pub snapshots: Mutex<HashMap<String, HashMap<PathBuf, LockEntry>>>,
}

impl LiveInner {
    fn new() -> Self {
        Self {
            paused: AtomicBool::new(false),
            compact_busy: AtomicBool::new(false),
            allow_dstorage: AtomicBool::new(false),
            last_plan: Mutex::new(None),
            locked: Mutex::new(BTreeSet::new()),
            installs: Mutex::new(HashMap::new()),
            snapshots: Mutex::new(HashMap::new()),
        }
    }
}

#[derive(Clone)]
pub struct LiveHandle {
    inner: Arc<LiveInner>,
}

impl LiveHandle {
    pub fn start() -> Self {
        let inner = Arc::new(LiveInner::new());
        let worker = inner.clone();
        thread::Builder::new()
            .name("rusticgu-live-compact".into())
            .spawn(move || run_watch_loop(worker))
            .ok();
        Self { inner }
    }

    pub fn paused(&self) -> bool {
        self.inner.paused.load(Ordering::SeqCst)
    }

    pub fn set_paused(&self, paused: bool) {
        self.inner.paused.store(paused, Ordering::SeqCst);
    }

    pub fn toggle_paused(&self) -> bool {
        let next = !self.paused();
        self.set_paused(next);
        next
    }

    pub fn set_allow_dstorage(&self, allow: bool) {
        self.inner.allow_dstorage.store(allow, Ordering::SeqCst);
    }

    pub fn set_compact_busy(&self, busy: bool) {
        self.inner.compact_busy.store(busy, Ordering::SeqCst);
    }

    pub fn is_locked(&self, title_id: &str) -> bool {
        self.inner
            .locked
            .lock()
            .map(|g| g.contains(title_id))
            .unwrap_or(false)
    }

    pub fn last_plan(&self) -> Option<StoredPlan> {
        self.inner.last_plan.lock().ok().and_then(|g| g.clone())
    }

    pub fn sync_titles(&self, titles: &[LibraryTitle]) {
        let mut map = HashMap::new();
        for title in titles {
            if let Some(app_id) = title.steam_app_id() {
                map.insert(app_id.to_string(), title.install_path.clone());
            }
        }
        if let Ok(mut installs) = self.inner.installs.lock() {
            *installs = map;
        }
    }

    pub fn recompact_last_patch(&self) -> Result<String, String> {
        let stored = self
            .last_plan()
            .ok_or_else(|| "No recent patch to recompact.".to_string())?;
        if self.is_locked(&stored.plan.title_id) {
            return Err("That title is still patching. Compact is locked.".into());
        }
        apply_live_plan(
            &stored.install,
            &stored.plan,
            self.inner.allow_dstorage.load(Ordering::SeqCst),
        )
        .map(|done| done.message)
    }

    #[cfg(test)]
    pub(crate) fn for_tests() -> Self {
        Self {
            inner: Arc::new(LiveInner::new()),
        }
    }

    #[cfg(test)]
    pub(crate) fn lock_title(&self, title_id: &str) {
        if let Ok(mut locked) = self.inner.locked.lock() {
            locked.insert(title_id.to_string());
        }
    }
}

fn run_watch_loop(inner: Arc<LiveInner>) {
    let compact = AppCompactor {
        inner: inner.clone(),
    };
    let inventory = AppInventory {
        inner: inner.clone(),
    };
    let mut watch = LiveWatch::new(DiskSteamStatus, compact, inventory);
    loop {
        let poll = watch.recommended_poll_secs().max(1);
        match watch.tick() {
            Ok(events) => {
                let any_patching = events.iter().any(|e| {
                    matches!(e, TickEvent::Locked { .. } | TickEvent::StayedLocked { .. })
                });
                if !any_patching {
                    watch.fs_watch.subscribe_idle_only(false);
                }
            }
            Err(_) => {}
        }
        thread::sleep(Duration::from_secs(poll));
    }
}

struct DiskSteamStatus;

impl SteamStatus for DiskSteamStatus {
    fn snapshot(&self) -> WatchResult<Vec<TitleStatus>> {
        let Some(steam) = steam_path() else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for folder in collect_library_folders(&steam) {
            let steamapps = folder.join("steamapps");
            let Ok(entries) = fs::read_dir(&steamapps) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                let Some(app_id) = name
                    .strip_prefix("appmanifest_")
                    .and_then(|name| name.strip_suffix(".acf"))
                    .and_then(|name| name.parse::<u32>().ok())
                else {
                    continue;
                };
                if seen.contains(&app_id) {
                    continue;
                }
                let acf = entry.path();
                let Ok(text) = fs::read_to_string(&acf) else {
                    continue;
                };
                let downloading = downloading_folder_present(&folder, app_id);
                if let Ok(status) = title_from_acf_text(&acf, &text, downloading) {
                    seen.insert(app_id);
                    out.push(status);
                }
            }
        }
        Ok(out)
    }
}

struct AppInventory {
    inner: Arc<LiveInner>,
}

impl FileInventory for AppInventory {
    fn list_install_files(&self, title_id: &str) -> WatchResult<Vec<InstallFile>> {
        let install = self
            .inner
            .installs
            .lock()
            .ok()
            .and_then(|m| m.get(title_id).cloned());
        let Some(root) = install else {
            return Ok(Vec::new());
        };
        let snapshot = self
            .inner
            .snapshots
            .lock()
            .ok()
            .and_then(|m| m.get(title_id).cloned());
        let mut files = Vec::new();
        if let Some(snapshot) = snapshot.as_ref() {
            for (rel, entry) in snapshot {
                let path = root.join(rel);
                let Ok(metadata) = fs::metadata(&path) else {
                    continue;
                };
                if !metadata.is_file() {
                    continue;
                }
                let len = metadata.len();
                let compact = match entry.compact {
                    FileCompactState::Compressed if entry.len == len => entry.compact,
                    _ => file_compact_state(&path),
                };
                files.push(InstallFile {
                    relative_path: rel.clone(),
                    compact,
                    appeared_after_lock: false,
                });
            }
        }
        let delta = collect_included_files(&root);
        for path in delta {
            if should_skip(&path) {
                continue;
            }
            let rel = path.strip_prefix(&root).unwrap_or(&path).to_path_buf();
            let is_baseline = snapshot.as_ref().is_some_and(|s| s.contains_key(&rel));
            if is_baseline {
                continue;
            }
            files.push(InstallFile {
                relative_path: rel,
                compact: file_compact_state(&path),
                appeared_after_lock: snapshot.is_some(),
            });
        }
        Ok(files)
    }
}

struct AppCompactor {
    inner: Arc<LiveInner>,
}

impl Compactor for AppCompactor {
    fn lock_compact(&mut self, title_id: &str) -> WatchResult<()> {
        if let Ok(mut locked) = self.inner.locked.lock() {
            locked.insert(title_id.to_string());
        }
        snapshot_lock(&self.inner, title_id);
        Ok(())
    }

    fn unlock_compact(&mut self, title_id: &str) -> WatchResult<()> {
        if let Ok(mut locked) = self.inner.locked.lock() {
            locked.remove(title_id);
        }
        if let Ok(mut snapshots) = self.inner.snapshots.lock() {
            snapshots.remove(title_id);
        }
        Ok(())
    }

    fn incremental_recompact(&mut self, plan: &IncrementalPlan) -> WatchResult<()> {
        let install = self
            .inner
            .installs
            .lock()
            .ok()
            .and_then(|m| m.get(&plan.title_id).cloned());
        if let Some(install) = install.clone() {
            if let Ok(mut last) = self.inner.last_plan.lock() {
                *last = Some(StoredPlan {
                    plan: plan.clone(),
                    install: install.clone(),
                });
            }
        }
        if !live_compact_should_apply(
            self.inner.paused.load(Ordering::SeqCst),
            self.inner.compact_busy.load(Ordering::SeqCst),
        ) {
            return Ok(());
        }
        let Some(install) = install else {
            return Ok(());
        };
        if title_path_is_excluded(&install, &plan.title_id) {
            return Ok(());
        }
        let allow = self.inner.allow_dstorage.load(Ordering::SeqCst);
        let _ = apply_live_plan(&install, plan, allow);
        Ok(())
    }
}

fn snapshot_lock(inner: &LiveInner, title_id: &str) {
    let install = inner
        .installs
        .lock()
        .ok()
        .and_then(|m| m.get(title_id).cloned());
    let Some(root) = install else {
        return;
    };
    let baseline: HashMap<PathBuf, LockEntry> = collect_included_files(&root)
        .into_iter()
        .filter_map(|path| {
            let relative_path = path.strip_prefix(&root).ok()?.to_path_buf();
            let len = fs::metadata(&path).ok()?.len();
            Some((
                relative_path,
                LockEntry {
                    compact: file_compact_state(&path),
                    len,
                },
            ))
        })
        .collect();
    if let Ok(mut snaps) = inner.snapshots.lock() {
        snaps.insert(title_id.to_string(), baseline);
    }
}

fn title_path_is_excluded(install: &Path, steam_app_id: &str) -> bool {
    if path_is_auto_excluded(install) {
        return true;
    }
    default_denylist()
        .match_title("", Some("steam"), Some(steam_app_id), Some(install))
        .is_some()
}

pub fn live_compact_should_apply(paused: bool, compact_busy: bool) -> bool {
    !paused && !compact_busy
}

fn apply_live_plan(
    install: &Path,
    plan: &IncrementalPlan,
    allow_dstorage: bool,
) -> Result<crate::compact::CompactResult, String> {
    let invs = build_incremental_invocations(install, &plan.files, CompactAlgorithm::Xpress8k);
    for inv in &invs {
        if invocation_has_force_flag(inv) {
            return Err("incremental recompact must not use /F".into());
        }
        if invocation_recurses_install_root(inv, install) {
            return Err("incremental recompact must not /S the install root".into());
        }
    }
    apply_incremental(
        install,
        &plan.files,
        CompactAlgorithm::Xpress8k,
        allow_dstorage,
        |_| {},
    )
}

fn file_compact_state(path: &Path) -> FileCompactState {
    #[cfg(windows)]
    {
        windows_file_compact_state(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        FileCompactState::Unknown
    }
}

#[cfg(windows)]
fn windows_file_compact_state(path: &Path) -> FileCompactState {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetCompressedFileSizeW;

    let Ok(meta) = fs::metadata(path) else {
        return FileCompactState::Unknown;
    };
    let logical = meta.len();
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut high = 0u32;
    let low = unsafe { GetCompressedFileSizeW(PCWSTR(wide.as_ptr()), Some(&mut high)) };
    if low == u32::MAX {
        return FileCompactState::Unknown;
    }
    let on_disk = ((high as u64) << 32) | u64::from(low);
    if on_disk < logical {
        FileCompactState::Compressed
    } else {
        FileCompactState::Uncompressed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use watch::{
        CompactEvent, IncrementalPlan, InstallFile, LiveWatch, MemoryInventory, MemorySteam,
        RecordingCompactor, TitleStatus, DOWNLOADING, FULLY_INSTALLED,
    };

    struct GatedCompactor {
        paused: bool,
        inner: RecordingCompactor,
        last: Option<IncrementalPlan>,
    }

    impl Compactor for GatedCompactor {
        fn lock_compact(&mut self, title_id: &str) -> WatchResult<()> {
            self.inner.lock_compact(title_id)
        }
        fn unlock_compact(&mut self, title_id: &str) -> WatchResult<()> {
            self.inner.unlock_compact(title_id)
        }
        fn incremental_recompact(&mut self, plan: &IncrementalPlan) -> WatchResult<()> {
            self.last = Some(plan.clone());
            if !live_compact_should_apply(self.paused, false) {
                return Ok(());
            }
            self.inner.incremental_recompact(plan)
        }
    }

    fn title(flags: u32) -> TitleStatus {
        TitleStatus {
            app_id: 570,
            name: "Dota 2".into(),
            install_dir: PathBuf::from("dota 2 beta"),
            state_flags: flags,
            bytes_to_download: 0,
            bytes_downloaded: 0,
            steam_downloading: false,
        }
    }

    fn inventory() -> MemoryInventory {
        let mut inv = MemoryInventory::default();
        inv.files.insert(
            "570".into(),
            vec![
                InstallFile {
                    relative_path: PathBuf::from("game.exe"),
                    compact: FileCompactState::Compressed,
                    appeared_after_lock: false,
                },
                InstallFile {
                    relative_path: PathBuf::from("new_patch.vpk"),
                    compact: FileCompactState::Uncompressed,
                    appeared_after_lock: true,
                },
            ],
        );
        inv
    }

    #[test]
    fn pause_gate_skips_incremental_apply() {
        assert!(live_compact_should_apply(false, false));
        assert!(!live_compact_should_apply(true, false));
        assert!(!live_compact_should_apply(false, true));

        let mut steam = MemorySteam::new(vec![title(FULLY_INSTALLED | DOWNLOADING)]);
        let compact = GatedCompactor {
            paused: true,
            inner: RecordingCompactor::default(),
            last: None,
        };
        let mut watch = LiveWatch::new(steam.clone(), compact, inventory());
        let _ = watch.tick().unwrap();
        steam.set_flags(570, FULLY_INSTALLED);
        steam.set_downloading_probe(570, false);
        watch.status = steam;
        let events = watch.tick().unwrap();
        assert!(events
            .iter()
            .any(|e| matches!(e, TickEvent::Unlocked { .. })));
        assert!(watch.compact.last.is_some());
        assert!(
            watch.compact.inner.incrementals().is_empty(),
            "paused Live Compact must not apply incremental WOF"
        );
    }

    #[test]
    fn unpaused_incremental_does_not_request_force() {
        let mut steam = MemorySteam::new(vec![title(FULLY_INSTALLED | DOWNLOADING)]);
        let compact = GatedCompactor {
            paused: false,
            inner: RecordingCompactor::default(),
            last: None,
        };
        let mut watch = LiveWatch::new(steam.clone(), compact, inventory());
        let _ = watch.tick().unwrap();
        steam.set_flags(570, FULLY_INSTALLED);
        steam.set_downloading_probe(570, false);
        watch.status = steam;
        let _ = watch.tick().unwrap();
        let incrementals = watch.compact.inner.incrementals();
        assert!(!incrementals.is_empty());
        for ev in incrementals {
            assert!(!ev.is_force_full_tree());
            if let CompactEvent::Incremental { files, .. } = ev {
                assert!(files.iter().any(|f| f.ends_with("new_patch.vpk")));
                assert!(!files.iter().any(|f| f.ends_with("game.exe")));
            }
        }
    }

    static STEAM_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn product_source() -> &'static str {
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/live.rs"))
            .split("#[cfg(test)]\nmod tests")
            .next()
            .unwrap()
    }

    fn product_section(start_marker: &str, end_marker: &str) -> &'static str {
        let source = product_source();
        let start = source.find(start_marker).unwrap();
        let source = &source[start..];
        let end = source.find(end_marker).unwrap();
        &source[..end]
    }

    struct SteamEnv {
        home: Option<std::ffi::OsString>,
        userprofile: Option<std::ffi::OsString>,
    }

    impl Drop for SteamEnv {
        fn drop(&mut self) {
            if let Some(home) = self.home.take() {
                std::env::set_var("HOME", home);
            } else {
                std::env::remove_var("HOME");
            }
            if let Some(userprofile) = self.userprofile.take() {
                std::env::set_var("USERPROFILE", userprofile);
            } else {
                std::env::remove_var("USERPROFILE");
            }
        }
    }

    fn with_steam_home<T>(home: &Path, f: impl FnOnce() -> T) -> T {
        let _lock = STEAM_ENV_LOCK.lock().unwrap();
        let _env = SteamEnv {
            home: std::env::var_os("HOME"),
            userprofile: std::env::var_os("USERPROFILE"),
        };
        std::env::set_var("HOME", home);
        std::env::set_var("USERPROFILE", home);
        let steam = home.join(".steam").join("steam");
        let _steam_root = crate::library::set_test_steam_root(&steam);
        f()
    }

    fn steam_fixture(tag: &str) -> (PathBuf, PathBuf) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!(
            "rusticgu-live-{tag}-home-{}-{stamp}",
            std::process::id()
        ));
        let steam = home.join(".steam").join("steam");
        let steamapps = steam.join("steamapps");
        std::fs::create_dir_all(&steamapps).unwrap();
        std::fs::write(
            steamapps.join("appmanifest_570.acf"),
            r#"
"AppState"
{
	"appid"		"570"
	"name"		"Dota 2"
	"StateFlags"		"1048576"
	"installdir"		"dota 2 beta"
	"BytesToDownload"		"100"
	"BytesDownloaded"		"1"
}
"#,
        )
        .unwrap();
        (home, steam)
    }

    fn install_fixture(tag: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "rusticgu-live-{tag}-install-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("game.exe"), b"game").unwrap();
        root
    }

    #[test]
    fn live_snapshot_reads_each_acf_once() {
        let snapshot = product_section(
            "impl SteamStatus for DiskSteamStatus",
            "struct AppInventory",
        );
        assert!(snapshot.contains("DiskSteamStatus"));
        assert!(
            !snapshot.contains("scan_library_folder"),
            "scan_library_folder must not be on the DiskSteamStatus snapshot path"
        );
        assert_eq!(
            snapshot.matches("fs::read_to_string").count(),
            1,
            "DiskSteamStatus must read each ACF with read_to_string once"
        );
        assert!(snapshot.contains("title_from_acf_text"));

        let (home, steam) = steam_fixture("snapshot");
        let fixture = steam.join("steamapps").join("appmanifest_570.acf");
        assert!(
            fixture.is_file(),
            "fixture appmanifest_570.acf must exist at {}",
            fixture.display()
        );
        let statuses = with_steam_home(&home, || {
            let resolved_steam = steam_path();
            assert_eq!(resolved_steam, Some(steam.clone()));
            assert_ne!(
                resolved_steam,
                std::env::var_os("HOME").map(PathBuf::from),
                "Steam fixture must not rely only on HOME"
            );
            assert_ne!(
                resolved_steam,
                std::env::var_os("USERPROFILE").map(PathBuf::from),
                "Steam fixture must not rely only on USERPROFILE"
            );
            DiskSteamStatus.snapshot()
        })
        .unwrap();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].app_id, 570);
        assert_eq!(
            statuses[0].state_flags, DOWNLOADING,
            "StateFlags / DOWNLOADING patching signal must survive the snapshot"
        );
        assert!(statuses[0].is_patching());
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn unlock_inventory_reuses_lock_baseline() {
        let product = product_source();
        assert!(product.contains("struct LockEntry"));
        assert!(product.contains("FileCompactState::Compressed"));
        assert!(product.contains("appeared_after_lock"));
        let inventory_source =
            product_section("impl FileInventory for AppInventory", "struct AppCompactor");
        assert!(
            inventory_source.contains("let delta = collect_included_files(&root)"),
            "collect_included_files must be delta-only after snapshot_lock"
        );
        assert_eq!(
            inventory_source
                .matches("collect_included_files(&root)")
                .count(),
            1,
            "collect_included_files is not a second full inventory on unlock"
        );
        assert!(inventory_source.contains("entry.compact"));
        assert!(inventory_source.contains("entry.len"));

        let inventory_fixture = inventory();
        let lock_baseline = inventory_fixture
            .files
            .get("570")
            .unwrap()
            .iter()
            .find(|file| !file.appeared_after_lock)
            .unwrap();
        assert_eq!(
            lock_baseline.compact,
            FileCompactState::Compressed,
            "FileCompactState::Compressed is a reusable lock baseline"
        );

        let root = install_fixture("unlock");
        let live = LiveHandle::for_tests();
        live.inner
            .installs
            .lock()
            .unwrap()
            .insert("570".into(), root.clone());
        snapshot_lock(&live.inner, "570");
        let inventory = AppInventory {
            inner: live.inner.clone(),
        };
        let before = inventory.list_install_files("570").unwrap();
        assert!(before.iter().any(|file| {
            file.relative_path == PathBuf::from("game.exe") && !file.appeared_after_lock
        }));

        let new_patch = root.join("new_patch.vpk");
        std::fs::write(&new_patch, b"patch").unwrap();
        let after = inventory.list_install_files("570").unwrap();
        let patch = after
            .iter()
            .find(|file| file.relative_path == PathBuf::from("new_patch.vpk"))
            .unwrap();
        assert!(
            patch.appeared_after_lock,
            "new_patch.vpk must be marked appeared_after_lock"
        );
        assert!(after.iter().any(|file| {
            file.relative_path == PathBuf::from("game.exe") && !file.appeared_after_lock
        }));

        let mut compactor = AppCompactor {
            inner: live.inner.clone(),
        };
        compactor.unlock_compact("570").unwrap();
        assert!(live.inner.snapshots.lock().unwrap().get("570").is_none());
        assert!(
            !live_compact_should_apply(false, true),
            "compact_busy must gate apply"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
