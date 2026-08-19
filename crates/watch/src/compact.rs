use std::path::PathBuf;

use crate::error::WatchResult;

/// Compact state of one file in an install tree.
/// The real WOF query lives in Engineer 3's crate; we only consume this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileCompactState {
    Uncompressed,
    Compressed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallFile {
    pub relative_path: PathBuf,
    pub compact: FileCompactState,
    /// Appeared after we locked the title (patch wrote a new file).
    pub appeared_after_lock: bool,
}

/// Files that should be incrementally recompressed.
///
/// Never represents `compact /F` / a full-tree force. Callers submit only
/// the named paths that are new or currently uncompressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncrementalPlan {
    pub title_id: String,
    pub files: Vec<PathBuf>,
}

impl IncrementalPlan {
    pub fn from_inventory(title_id: impl Into<String>, files: &[InstallFile]) -> Self {
        let files = files
            .iter()
            .filter(|f| {
                f.appeared_after_lock || matches!(f.compact, FileCompactState::Uncompressed)
            })
            .map(|f| f.relative_path.clone())
            .collect();
        Self {
            title_id: title_id.into(),
            files,
        }
    }
}

/// Sink that records lock / unlock / incremental-recompact decisions.
/// Engineer 3 owns the actual WOF / `compact.exe` work.
pub trait Compactor {
    fn lock_compact(&mut self, title_id: &str) -> WatchResult<()>;
    fn unlock_compact(&mut self, title_id: &str) -> WatchResult<()>;
    fn incremental_recompact(&mut self, plan: &IncrementalPlan) -> WatchResult<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactEvent {
    Lock(String),
    Unlock(String),
    Incremental {
        title_id: String,
        files: Vec<PathBuf>,
    },
}

impl CompactEvent {
    pub fn is_force_full_tree(&self) -> bool {
        false
    }
}

/// Test spy. Never issues `/F`.
#[derive(Debug, Default, Clone)]
pub struct RecordingCompactor {
    pub events: Vec<CompactEvent>,
}

impl Compactor for RecordingCompactor {
    fn lock_compact(&mut self, title_id: &str) -> WatchResult<()> {
        self.events.push(CompactEvent::Lock(title_id.to_string()));
        Ok(())
    }

    fn unlock_compact(&mut self, title_id: &str) -> WatchResult<()> {
        self.events.push(CompactEvent::Unlock(title_id.to_string()));
        Ok(())
    }

    fn incremental_recompact(&mut self, plan: &IncrementalPlan) -> WatchResult<()> {
        self.events.push(CompactEvent::Incremental {
            title_id: plan.title_id.clone(),
            files: plan.files.clone(),
        });
        Ok(())
    }
}

impl RecordingCompactor {
    pub fn locks(&self) -> Vec<&str> {
        self.events
            .iter()
            .filter_map(|e| match e {
                CompactEvent::Lock(id) => Some(id.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn incrementals(&self) -> Vec<&CompactEvent> {
        self.events
            .iter()
            .filter(|e| matches!(e, CompactEvent::Incremental { .. }))
            .collect()
    }
}

/// Install-file listing used to decide *what* to recompact.
pub trait FileInventory {
    fn list_install_files(&self, title_id: &str) -> WatchResult<Vec<InstallFile>>;
}

#[derive(Debug, Default, Clone)]
pub struct MemoryInventory {
    pub files: std::collections::BTreeMap<String, Vec<InstallFile>>,
}

impl FileInventory for MemoryInventory {
    fn list_install_files(&self, title_id: &str) -> WatchResult<Vec<InstallFile>> {
        Ok(self.files.get(title_id).cloned().unwrap_or_default())
    }
}

impl<T: Compactor + ?Sized> Compactor for &mut T {
    fn lock_compact(&mut self, title_id: &str) -> WatchResult<()> {
        (**self).lock_compact(title_id)
    }
    fn unlock_compact(&mut self, title_id: &str) -> WatchResult<()> {
        (**self).unlock_compact(title_id)
    }
    fn incremental_recompact(&mut self, plan: &IncrementalPlan) -> WatchResult<()> {
        (**self).incremental_recompact(plan)
    }
}

impl<T: FileInventory + ?Sized> FileInventory for &T {
    fn list_install_files(&self, title_id: &str) -> WatchResult<Vec<InstallFile>> {
        (**self).list_install_files(title_id)
    }
}
