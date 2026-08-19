use std::collections::BTreeSet;

use crate::compact::{Compactor, FileInventory, IncrementalPlan};
use crate::error::WatchResult;
use crate::flags::{ACTIVE_POLL_INTERVAL_SECS, IDLE_POLL_INTERVAL_SECS};
use crate::status::{SteamStatus, TitleStatus};

/// Filesystem-event subscription gate.
///
/// FS events during a Steam patch are noisy and unsafe. This watcher is
/// poll-only while any title is locked; tests assert we do not subscribe
/// mid-patch.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FsWatchGate {
    subscribed: bool,
    subscribe_attempts: u32,
}

impl FsWatchGate {
    pub fn is_subscribed(&self) -> bool {
        self.subscribed
    }

    pub fn subscribe_attempts(&self) -> u32 {
        self.subscribe_attempts
    }

    /// Allowed only when nothing is patching.
    pub fn subscribe_idle_only(&mut self, any_patching: bool) {
        self.subscribe_attempts += 1;
        if !any_patching {
            self.subscribed = true;
        }
    }

    pub fn unsubscribe(&mut self) {
        self.subscribed = false;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TickEvent {
    Locked { title_id: String },
    StayedLocked { title_id: String },
    IncrementalRecompact { title_id: String, files: usize },
    Unlocked { title_id: String },
}

/// Live Compact watcher. Drive it with [`LiveWatch::tick`] — no thread sleep.
pub struct LiveWatch<S, C, I> {
    pub status: S,
    pub compact: C,
    pub inventory: I,
    locked: BTreeSet<String>,
    pub fs_watch: FsWatchGate,
}

impl<S, C, I> LiveWatch<S, C, I>
where
    S: SteamStatus,
    C: Compactor,
    I: FileInventory,
{
    pub fn new(status: S, compact: C, inventory: I) -> Self {
        Self {
            status,
            compact,
            inventory,
            locked: BTreeSet::new(),
            fs_watch: FsWatchGate::default(),
        }
    }

    pub fn is_locked(&self, title_id: &str) -> bool {
        self.locked.contains(title_id)
    }

    pub fn recommended_poll_secs(&self) -> u64 {
        if self.locked.is_empty() {
            IDLE_POLL_INTERVAL_SECS
        } else {
            ACTIVE_POLL_INTERVAL_SECS
        }
    }

    pub fn tick(&mut self) -> WatchResult<Vec<TickEvent>> {
        let snap = self.status.snapshot()?;
        let any_patching = snap.iter().any(TitleStatus::is_patching);
        // Never keep (or take) an FS-watcher subscription during a patch.
        if any_patching {
            self.fs_watch.unsubscribe();
        }

        let mut events = Vec::new();
        let mut seen = BTreeSet::new();

        for title in &snap {
            let id = title.title_id();
            seen.insert(id.clone());

            if title.is_patching() {
                if self.locked.insert(id.clone()) {
                    self.compact.lock_compact(&id)?;
                    events.push(TickEvent::Locked { title_id: id });
                } else {
                    events.push(TickEvent::StayedLocked { title_id: id });
                }
            } else if self.locked.contains(&id) {
                let files = self.inventory.list_install_files(&id)?;
                let plan = IncrementalPlan::from_inventory(&id, &files);
                if !plan.files.is_empty() {
                    let n = plan.files.len();
                    self.compact.incremental_recompact(&plan)?;
                    events.push(TickEvent::IncrementalRecompact {
                        title_id: id.clone(),
                        files: n,
                    });
                }
                self.compact.unlock_compact(&id)?;
                self.locked.remove(&id);
                events.push(TickEvent::Unlocked { title_id: id });
            }
        }

        // Titles that vanished from the snapshot while locked: unlock without /F.
        let stale: Vec<String> = self
            .locked
            .iter()
            .filter(|id| !seen.contains(*id))
            .cloned()
            .collect();
        for id in stale {
            self.compact.unlock_compact(&id)?;
            self.locked.remove(&id);
            events.push(TickEvent::Unlocked { title_id: id });
        }

        Ok(events)
    }
}
