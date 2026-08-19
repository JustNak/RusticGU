//! Compression policy for the "shelf".
//!
//! - Cold / never-played titles → **LZX**
//! - Recently played titles stay **XPRESS8K**
//! - Launching a shelved (LZX) title walks back to **XPRESS**
//! - GW2-class self-rewriters are **excluded** from any compact
//!
//! Thresholds are named constants / [`ShelfConfig`] with tested defaults.

pub mod denylist;
pub mod policy;
pub mod thresholds;

pub use denylist::{default_denylist, DenyList, DenyRule};
pub use policy::{recommend, recommend_default, CompactPolicy, PolicyInput};
pub use thresholds::{Recency, ShelfConfig, DEFAULT_COLD_AFTER, DEFAULT_RECENT_WITHIN};
