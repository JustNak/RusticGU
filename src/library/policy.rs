//! Shelf policy adapter. LZX is Shelf-only; live compact stays XPRESS8K.

use std::time::{Duration, SystemTime};

use shelf::{recommend_default, CompactPolicy, PolicyInput};

use super::title::LibraryTitle;
use crate::compact::{path_is_auto_excluded, title_is_auto_excluded};
use crate::settings::CompactAlgorithm;

pub fn last_played_system_time(unix: Option<u64>) -> Option<SystemTime> {
    unix.and_then(|secs| SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(secs)))
}

pub fn shelf_policy_for(title: &LibraryTitle, is_launching: bool) -> CompactPolicy {
    let last_played = last_played_system_time(title.last_played_unix);
    let resting_input = PolicyInput {
        title: &title.name,
        last_played,
        is_launching: false,
        store_id: Some(title.store.as_str()),
        launcher_id: title.launcher_id.as_deref(),
        install_folder: Some(title.install_path.as_path()),
        currently_shelved_lzx: false,
    };
    let resting = recommend_default(&resting_input, SystemTime::now());
    let currently_shelved_lzx = matches!(resting, CompactPolicy::Lzx);
    let input = PolicyInput {
        title: &title.name,
        last_played,
        is_launching,
        store_id: Some(title.store.as_str()),
        launcher_id: title.launcher_id.as_deref(),
        install_folder: Some(title.install_path.as_path()),
        currently_shelved_lzx,
    };
    recommend_default(&input, SystemTime::now())
}

pub fn algorithm_from_policy(
    policy: &CompactPolicy,
    live_fallback: CompactAlgorithm,
) -> Option<CompactAlgorithm> {
    match policy {
        CompactPolicy::Exclude { .. } => None,
        CompactPolicy::Lzx => Some(CompactAlgorithm::Lzx),
        CompactPolicy::Xpress8k => Some(live_fallback.for_live_library()),
        CompactPolicy::Xpress => Some(CompactAlgorithm::Xpress),
    }
}

pub fn title_is_compact_excluded(title: &LibraryTitle) -> bool {
    if title_is_auto_excluded(&title.name) || path_is_auto_excluded(&title.install_path) {
        return true;
    }
    matches!(
        shelf_policy_for(title, false),
        CompactPolicy::Exclude { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    use shelf::{recommend_default, CompactPolicy, PolicyInput};

    fn title(name: &str, last_played_unix: Option<u64>) -> LibraryTitle {
        LibraryTitle {
            id: format!("steam:{name}"),
            name: name.into(),
            install_path: PathBuf::from(r"D:\Steam\steamapps\common\Game"),
            store: super::super::title::LibraryStore::Steam,
            launcher_id: Some("1".into()),
            last_played_unix,
            logical_bytes: None,
            on_disk_bytes: None,
            steam_app_id: Some(1),
            steam_library_path: None,
            steam_install_dir_name: None,
        }
    }

    #[test]
    fn unknown_last_played_is_lzx() {
        let policy = shelf_policy_for(&title("Hades", None), false);
        assert_eq!(policy, CompactPolicy::Lzx);
        assert_eq!(
            algorithm_from_policy(&policy, CompactAlgorithm::Xpress8k),
            Some(CompactAlgorithm::Lzx)
        );
    }

    #[test]
    fn recent_last_played_is_xpress8k() {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(1_800_000_000);
        let recent = now.saturating_sub(2 * 86400);
        let policy = shelf_policy_for(&title("Hades", Some(recent)), false);
        assert_eq!(policy, CompactPolicy::Xpress8k);
        assert_eq!(
            algorithm_from_policy(&policy, CompactAlgorithm::Xpress8k),
            Some(CompactAlgorithm::Xpress8k)
        );
    }

    #[test]
    fn launch_of_unknown_walks_back_to_xpress() {
        let policy = shelf_policy_for(&title("Hades", None), true);
        assert_eq!(policy, CompactPolicy::Xpress);
        assert_eq!(
            algorithm_from_policy(&policy, CompactAlgorithm::Xpress8k),
            Some(CompactAlgorithm::Xpress)
        );
    }

    #[test]
    fn recommend_default_unknown_is_lzx_recent_is_xpress() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let mut unknown = PolicyInput::new("Celeste");
        unknown.last_played = None;
        assert_eq!(recommend_default(&unknown, now), CompactPolicy::Lzx);

        let mut recent = PolicyInput::new("Celeste");
        recent.last_played = Some(now - Duration::from_secs(86400));
        assert_eq!(recommend_default(&recent, now), CompactPolicy::Xpress8k);
    }

    #[test]
    fn gw2_is_excluded_by_shelf_and_app() {
        let gw2 = title("Guild Wars 2", None);
        assert!(title_is_compact_excluded(&gw2));
        assert!(matches!(
            shelf_policy_for(&gw2, false),
            CompactPolicy::Exclude { .. }
        ));
    }
}
