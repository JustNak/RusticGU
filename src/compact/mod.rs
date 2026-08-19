//! WOF CompactOS (`compact /EXE`) engine.
//!
//! Never uses NTFS LZNT1 (`compact` without `/EXE`). Undo is `/U /EXE` only.

mod command;
mod engine;
mod skip;

#[allow(unused_imports)]
pub use command::{apply_target_paths, build_apply_invocations, build_compact_command, CompactOp};
#[allow(unused_imports)]
pub use command::{
    invocation_recurses_install_root, is_lznt1_command, is_wof_exe_command, CompactInvocation,
};
pub use engine::{
    apply_compact, estimate_compact, preflight, CompactEstimate, CompactProgress, CompactRefuse,
};
#[allow(unused_imports)]
pub use engine::{
    is_windows_apps_path, os_supports_wof, running_exe_in_tree, volume_filesystem, CompactResult,
};
#[allow(unused_imports)]
pub use skip::{
    path_is_auto_excluded, should_skip, skip_reason, title_is_auto_excluded, tree_contains_dstorage,
};
