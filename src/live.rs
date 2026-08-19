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
    appmanifest_path, collect_library_folders, downloading_folder_present, scan_library_folder,
    steam_path, LibraryTitle,
};
use crate::settings::CompactAlgorithm;
use shelf::default_denylist;

#[derive(Debug, Clone)]
pub struct StoredPlan {
    pub plan: IncrementalPlan,
    pub install: PathBuf,
}

pub struct LiveInner {
    pub paused: AtomicBool,
    pub compact_busy: AtomicBool,
    pub allow_dstorage: AtomicBool,
    pub last_plan: Mutex<Option<StoredPlan>>,
    pub locked: Mutex<BTreeSet<String>>,
    pub installs: Mutex<HashMap<String, PathBuf>>,
    pub snapshots: Mutex<HashMap<String, HashSet<PathBuf>>>,
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
        for folder in collect_library_folders(&steam) {
            for game in scan_library_folder(&folder) {
                let acf = appmanifest_path(&folder, game.app_id);
                let Ok(text) = fs::read_to_string(&acf) else {
                    continue;
                };
                let downloading = downloading_folder_present(&folder, game.app_id);
                if let Ok(status) = title_from_acf_text(&acf, &text, downloading) {
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
        for path in collect_included_files(&root) {
            if should_skip(&path) {
                continue;
            }
            let rel = path.strip_prefix(&root).unwrap_or(&path).to_path_buf();
            let appeared_after_lock = snapshot
                .as_ref()
                .map(|s| !s.contains(&rel))
                .unwrap_or(false);
            files.push(InstallFile {
                relative_path: rel,
                compact: file_compact_state(&path),
                appeared_after_lock,
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
    let rels: HashSet<PathBuf> = collect_included_files(&root)
        .into_iter()
        .filter_map(|p| p.strip_prefix(&root).ok().map(|r| r.to_path_buf()))
        .collect();
    if let Ok(mut snaps) = inner.snapshots.lock() {
        snaps.insert(title_id.to_string(), rels);
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
}
