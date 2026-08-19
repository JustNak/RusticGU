//! Game library discovery. This PR only implements Steam.
//!
//! Extra store scanners (Epic / GOG / EA / Ubisoft / Riot / Battle.net / itch)
//! are reserved for Engineer 4.

mod steam;
mod vdf;

#[allow(unused_imports)]
pub use steam::{collect_library_folders, game_from_acf, library_paths_from_vdf, steam_path};
pub use steam::{scan_steam_library, SteamGame};
#[allow(unused_imports)]
pub use vdf::{parse_vdf, VdfObject, VdfValue};
