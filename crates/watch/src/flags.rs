//! Steam `appmanifest_*.acf` `StateFlags` bitfield.
//!
//! Values match Open Steamworks `EAppState` (`AppsCommon.h`), which is what
//! the Steam client writes into ACF files. Common decimal combinations:
//!
//! | Value | Meaning |
//! |------:|---------|
//! | 4 | `FULLY_INSTALLED`: installed and idle |
//! | 6 | `FULLY_INSTALLED \| UPDATE_REQUIRED` |
//! | 1026 | `UPDATE_REQUIRED \| UPDATE_STARTED` (download just kicked off) |
//! | 4 \| 1048576 | installed + `DOWNLOADING` |
//!
//! # What we treat as "patching" (compact **locked**)
//!
//! Any of the following bits, **or** a downloading probe, **or**
//! `BytesToDownload > BytesDownloaded`:
//!
//! - `UPDATE_REQUIRED` (2)
//! - `FILES_MISSING` (32), `FILES_CORRUPT` (128)
//! - `UPDATE_RUNNING` (256), `UPDATE_PAUSED` (512), `UPDATE_STARTED` (1024)
//! - `UNINSTALLING` (2048)
//! - `RECONFIGURING` (65536), `VALIDATING` (131072)
//! - `ADDING_FILES` (262144), `PREALLOCATING` (524288)
//! - `DOWNLOADING` (1048576), `STAGING` (2097152), `COMMITTING` (4194304)
//! - `UPDATE_STOPPING` (8388608)
//!
//! # What we treat as idle (compact **unlocked**)
//!
//! `FULLY_INSTALLED` (4) with none of the patching bits above, no outstanding
//! download bytes, and the Steam downloading probe false.
//!
//! `APP_RUNNING` (8192) and `BACKUP_RUNNING` (4096) do **not** lock compact
//! by themselves; those are not patch/download states.
//!
//! Hypothesis confirmed against Open Steamworks: update/download/validating
//! bits lock; fully-installed-and-idle unlocks.

pub const UNINSTALLED: u32 = 1;
pub const UPDATE_REQUIRED: u32 = 2;
pub const FULLY_INSTALLED: u32 = 4;
pub const DATA_ENCRYPTED: u32 = 8;
pub const DATA_LOCKED: u32 = 16;
pub const FILES_MISSING: u32 = 32;
pub const SHARED_ONLY: u32 = 64;
pub const FILES_CORRUPT: u32 = 128;
pub const UPDATE_RUNNING: u32 = 256;
pub const UPDATE_PAUSED: u32 = 512;
pub const UPDATE_STARTED: u32 = 1024;
pub const UNINSTALLING: u32 = 2048;
pub const BACKUP_RUNNING: u32 = 4096;
pub const APP_RUNNING: u32 = 8192;
pub const RECONFIGURING: u32 = 65_536;
pub const VALIDATING: u32 = 131_072;
pub const ADDING_FILES: u32 = 262_144;
pub const PREALLOCATING: u32 = 524_288;
pub const DOWNLOADING: u32 = 1_048_576;
pub const STAGING: u32 = 2_097_152;
pub const COMMITTING: u32 = 4_194_304;
pub const UPDATE_STOPPING: u32 = 8_388_608;

/// Bits that mean "do not compact; title is mutating".
pub const PATCHING_BITS: u32 = UPDATE_REQUIRED
    | FILES_MISSING
    | FILES_CORRUPT
    | UPDATE_RUNNING
    | UPDATE_PAUSED
    | UPDATE_STARTED
    | UNINSTALLING
    | RECONFIGURING
    | VALIDATING
    | ADDING_FILES
    | PREALLOCATING
    | DOWNLOADING
    | STAGING
    | COMMITTING
    | UPDATE_STOPPING;

/// Idle poll while nothing is patching. Tests never sleep; they call `tick`.
pub const IDLE_POLL_INTERVAL_SECS: u64 = 30;
/// Faster poll while at least one title is locked.
pub const ACTIVE_POLL_INTERVAL_SECS: u64 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatchingSignals {
    pub state_flags: u32,
    pub bytes_to_download: u64,
    pub bytes_downloaded: u64,
    /// Separate Steam "something is downloading" probe (libraryfolders /
    /// downloading folder / API). There is no official download-complete hook.
    pub steam_downloading: bool,
}

impl PatchingSignals {
    pub fn is_patching(self) -> bool {
        if self.steam_downloading {
            return true;
        }
        if self.state_flags & PATCHING_BITS != 0 {
            return true;
        }
        self.bytes_to_download > 0 && self.bytes_downloaded < self.bytes_to_download
    }

    pub fn is_idle_installed(self) -> bool {
        self.state_flags & FULLY_INSTALLED != 0 && !self.is_patching()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_is_idle() {
        let s = PatchingSignals {
            state_flags: FULLY_INSTALLED,
            bytes_to_download: 0,
            bytes_downloaded: 0,
            steam_downloading: false,
        };
        assert!(!s.is_patching());
        assert!(s.is_idle_installed());
    }

    #[test]
    fn downloading_bit_locks() {
        let s = PatchingSignals {
            state_flags: FULLY_INSTALLED | DOWNLOADING,
            bytes_to_download: 100,
            bytes_downloaded: 10,
            steam_downloading: false,
        };
        assert!(s.is_patching());
    }

    #[test]
    fn update_started_1026_locks() {
        let s = PatchingSignals {
            state_flags: UPDATE_REQUIRED | UPDATE_STARTED,
            bytes_to_download: 0,
            bytes_downloaded: 0,
            steam_downloading: false,
        };
        assert_eq!(s.state_flags, 1026);
        assert!(s.is_patching());
    }
}
