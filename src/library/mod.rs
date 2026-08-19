//! Game library discovery. This PR only implements Steam.
//!
//! Extra store scanners (Epic / GOG / EA / Ubisoft / Riot / Battle.net / itch)
//! are reserved for Engineer 4.

mod steam;
mod vdf;

#[allow(unused_imports)]
pub use steam::{
    collect_library_folders, downloading_folder_present, game_from_acf, install_is_steam_updating,
    is_steam_title_updating, library_paths_from_vdf, state_flags_from_acf,
    state_flags_indicate_update, steam_path, steam_updating_app_id,
};
pub use steam::{scan_steam_library, SteamGame};
#[allow(unused_imports)]
pub use vdf::{parse_vdf, VdfObject, VdfValue};
