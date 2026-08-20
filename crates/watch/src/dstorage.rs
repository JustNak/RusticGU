//! DirectStorage **runtime file** hint.
//!
//! A positive check requires **both** `dstorage.dll` **and** `dstoragecore.dll`
//! in the provided filename set (typically the install root listing).
//!
//! # Caveats: do not over-claim
//!
//! - Presence of the DLLs **does not** mean the game actually uses DirectStorage.
//!   Titles may ship the redistributable unused, or load it only on some paths.
//! - This check **will miss**:
//!   - custom `DStorageSDKPath` (DLLs live outside the install root)
//!   - renamed DLLs
//!   - Game Pass / GDK layout (payloads under a different package tree)
//!
//! Treat [`DirectStorageHint::BothDllsPresent`] as a weak hint for operators,
//! not a capability proof.

use std::path::Path;

pub const DSTORAGE_DLL: &str = "dstorage.dll";
pub const DSTORAGE_CORE_DLL: &str = "dstoragecore.dll";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectStorageHint {
    /// Both official runtime DLLs were seen. **Not** proof of actual use.
    BothDllsPresent,
    /// Zero or one of the two DLLs, including custom/renamed/Game Pass misses.
    NotDetected,
}

/// Scan file names (not full trees). Matching is case-insensitive on the
/// final path component only.
pub fn detect_direct_storage<I, P>(names: I) -> DirectStorageHint
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let mut has_ds = false;
    let mut has_core = false;
    for p in names {
        let raw = p
            .as_ref()
            .file_name()
            .and_then(|n| n.to_str())
            .or_else(|| p.as_ref().to_str())
            .unwrap_or("");
        let file = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
        if file.eq_ignore_ascii_case(DSTORAGE_DLL) {
            has_ds = true;
        }
        if file.eq_ignore_ascii_case(DSTORAGE_CORE_DLL) {
            has_core = true;
        }
    }
    if has_ds && has_core {
        DirectStorageHint::BothDllsPresent
    } else {
        DirectStorageHint::NotDetected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_dlls_required() {
        assert_eq!(
            detect_direct_storage(["dstorage.dll"]),
            DirectStorageHint::NotDetected
        );
        assert_eq!(
            detect_direct_storage(["dstoragecore.dll"]),
            DirectStorageHint::NotDetected
        );
        assert_eq!(
            detect_direct_storage(["dstorage.dll", "dstoragecore.dll"]),
            DirectStorageHint::BothDllsPresent
        );
        assert_eq!(
            detect_direct_storage([r"C:\Game\DSTORAGE.DLL", r"C:\Game\DStorageCore.dll"]),
            DirectStorageHint::BothDllsPresent
        );
        assert_eq!(
            detect_direct_storage(["game.exe", "engine.dll"]),
            DirectStorageHint::NotDetected
        );
    }
}
