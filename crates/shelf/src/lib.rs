//! Compression policy for the "shelf".
//!
//! - Cold / **unknown last-played** (`None`) → **LZX** (conservative; never
//!   fabricate a timestamp; see [`last_played`])
//! - Recently played titles stay **XPRESS8K**
//! - Launching a shelved (LZX) title walks back to **XPRESS**
//! - Confirmed self-rewriters (GW2, Secret World Legends, LOTRO) are
//!   **excluded** from any compact
//!
//! Thresholds are named constants / [`ShelfConfig`] with tested defaults.

pub mod denylist;
pub mod last_played;
pub mod policy;
pub mod thresholds;

pub use denylist::{default_denylist, DenyList, DenyRule};
pub use last_played::{
    last_played_from_acf, last_played_from_itch_local_last_run_at,
    last_played_from_steam_localconfig, last_played_from_steam_userdata,
    last_played_unix_from_steam_localconfig, last_played_unix_from_steam_userdata,
    safe_last_played_source, LastPlayedSource,
};
pub use policy::{recommend, recommend_default, CompactPolicy, PolicyInput};
pub use thresholds::{Recency, ShelfConfig, DEFAULT_COLD_AFTER, DEFAULT_RECENT_WITHIN};
