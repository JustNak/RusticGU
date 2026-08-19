//! Both DirectStorage DLLs are required. Presence ≠ actual use.
//! Misses custom DStorageSDKPath, renamed DLLs, and Game Pass layout.

use watch::{detect_direct_storage, DirectStorageHint};

#[test]
fn both_dlls_required_for_positive_hint() {
    assert_eq!(
        detect_direct_storage(["dstorage.dll"]),
        DirectStorageHint::NotDetected,
        "one DLL is not enough (and does not prove DS is used)"
    );
    assert_eq!(
        detect_direct_storage(["dstorage.dll", "dstoragecore.dll"]),
        DirectStorageHint::BothDllsPresent
    );
}

#[test]
fn renamed_or_custom_path_is_a_false_negative() {
    assert_eq!(
        detect_direct_storage(["custom_dstorage.dll", "sdk/dstorage_renamed.dll"]),
        DirectStorageHint::NotDetected
    );
}
