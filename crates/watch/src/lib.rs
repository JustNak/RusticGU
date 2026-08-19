//! Live Compact watcher for Steam titles.
//!
//! There is **no** official Steam download-complete hook. This crate polls
//! ACF `StateFlags` (and download byte counters) plus an injected downloading
//! probe. While a title is patching it **locks** compact; after it leaves that
//! state it requests an **incremental** recompact of only new or uncompressed
//! files — never `compact /F`.
//!
//! The real WOF / `compact.exe` work lives in Engineer 3's crate. We only
//! decide *when* and *which paths*.
//!
//! See [`flags`] for the Open Steamworks `EAppState` bits treated as patching.

pub mod acf;
pub mod compact;
pub mod error;
pub mod flags;
pub mod machine;
pub mod status;

pub use compact::{
    CompactEvent, Compactor, FileCompactState, FileInventory, IncrementalPlan, InstallFile,
    MemoryInventory, RecordingCompactor,
};
pub use error::{WatchError, WatchResult};
pub use flags::{
    PatchingSignals, ACTIVE_POLL_INTERVAL_SECS, DOWNLOADING, FULLY_INSTALLED, IDLE_POLL_INTERVAL_SECS,
    PATCHING_BITS, UPDATE_REQUIRED, UPDATE_STARTED, VALIDATING,
};
pub use machine::{FsWatchGate, LiveWatch, TickEvent};
pub use status::{title_from_acf, title_from_acf_text, MemorySteam, SteamStatus, TitleStatus};
