//! WOF CompactOS (`compact /EXE`) engine.
//!
//! Never uses NTFS LZNT1 (`compact` without `/EXE`). Undo is `/U /EXE` only.

mod command;
mod engine;
mod skip;

#[allow(unused_imports)]
pub use command::{build_compact_command, CompactOp};
#[allow(unused_imports)]
pub use command::{is_lznt1_command, is_wof_exe_command, CompactInvocation};
pub use engine::{
    apply_compact, estimate_compact, preflight, CompactEstimate, CompactProgress, CompactRefuse,
};
#[allow(unused_imports)]
pub use engine::{
    is_windows_apps_path, os_supports_wof, running_exe_in_tree, volume_filesystem, CompactResult,
};
#[allow(unused_imports)]
pub use skip::{should_skip, skip_reason, tree_contains_dstorage};
