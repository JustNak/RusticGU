//! Game library discovery: Steam (this crate) plus extra launchers via `stores`.

mod policy;
mod scan;
mod steam;
mod title;
mod vdf;

#[allow(unused_imports)]
pub use steam::{
    appmanifest_path, cheap_install_sizes, scan_library_folder, scan_steam_library,
    sizes_indicate_compacted, SteamGame,
};
#[allow(unused_imports)]
pub use steam::{
    collect_library_folders, downloading_folder_present, game_from_acf, install_is_steam_updating,
    is_steam_title_updating, library_paths_from_vdf, state_flags_from_acf,
    state_flags_indicate_update, steam_path, steam_updating_app_id,
};
#[cfg(test)]
pub(crate) use steam::set_test_steam_root;
#[allow(unused_imports)]
pub use vdf::{parse_vdf, VdfObject, VdfValue};

#[allow(unused_imports)]
pub use policy::{
    algorithm_from_policy, last_played_system_time, shelf_policy_for, title_is_compact_excluded,
};
#[allow(unused_imports)]
pub use scan::{
    append_custom_titles, discover_extra_titles, extra_store_options, extra_store_roots,
    merge_library, scan_library, typical_xbox_games_roots, ScanOptions,
};
#[allow(unused_imports)]
pub use title::{custom_title_id, extra_title_id, steam_title_id, LibraryStore, LibraryTitle};
