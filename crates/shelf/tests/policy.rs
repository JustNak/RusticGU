use std::path::Path;
use std::time::{Duration, SystemTime};

use shelf::{
    default_denylist, recommend, CompactPolicy, DenyList, DenyRule, PolicyInput, ShelfConfig,
    DEFAULT_COLD_AFTER, DEFAULT_RECENT_WITHIN,
};

fn now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)
}

fn decide(input: PolicyInput<'_>) -> CompactPolicy {
    recommend(&input, now(), &ShelfConfig::default(), &default_denylist())
}

#[test]
fn cold_or_never_played_is_lzx() {
    let mut input = PolicyInput::new("Hades");
    input.last_played = None;
    assert_eq!(decide(input), CompactPolicy::Lzx);

    let mut input = PolicyInput::new("Hades");
    input.last_played = Some(now() - DEFAULT_COLD_AFTER - Duration::from_secs(3600));
    assert_eq!(decide(input), CompactPolicy::Lzx);
}

#[test]
fn recent_is_xpress8k() {
    let mut input = PolicyInput::new("Hades");
    input.last_played = Some(now() - Duration::from_secs(2 * 86400));
    assert_eq!(decide(input), CompactPolicy::Xpress8k);
    assert!(DEFAULT_RECENT_WITHIN >= Duration::from_secs(2 * 86400));
}

#[test]
fn warm_gap_stays_xpress8k() {
    let mut input = PolicyInput::new("Hades");
    input.last_played = Some(now() - Duration::from_secs(10 * 86400));
    assert_eq!(decide(input), CompactPolicy::Xpress8k);
}

#[test]
fn launch_of_lzx_shelved_walks_back_to_xpress() {
    let mut input = PolicyInput::new("Hades");
    input.last_played = Some(now() - DEFAULT_COLD_AFTER - Duration::from_secs(86400));
    input.currently_shelved_lzx = true;
    input.is_launching = true;
    assert_eq!(decide(input), CompactPolicy::Xpress);
}

#[test]
fn launch_of_recent_xpress8k_stays_xpress8k() {
    let mut input = PolicyInput::new("Hades");
    input.last_played = Some(now() - Duration::from_secs(86400));
    input.currently_shelved_lzx = false;
    input.is_launching = true;
    assert_eq!(decide(input), CompactPolicy::Xpress8k);
}

#[test]
fn gw2_is_excluded_case_insensitive() {
    for name in ["Guild Wars 2", "guild wars 2", "GUILD WARS 2", "GW2", "gw2"] {
        let policy = decide(PolicyInput::new(name));
        match policy {
            CompactPolicy::Exclude { reason } => {
                assert!(reason.to_ascii_lowercase().contains("guild wars"));
            }
            other => panic!("{name} should be excluded, got {other:?}"),
        }
    }
}

#[test]
fn secret_world_legends_and_lotro_are_excluded() {
    for name in ["Secret World Legends", "secret world legends"] {
        match decide(PolicyInput::new(name)) {
            CompactPolicy::Exclude { reason } => {
                assert!(reason.to_ascii_lowercase().contains("secret world"));
            }
            other => panic!("{name} should be excluded, got {other:?}"),
        }
    }
    for name in [
        "The Lord of the Rings Online",
        "LOTRO",
        "lord of the rings online",
    ] {
        match decide(PolicyInput::new(name)) {
            CompactPolicy::Exclude { reason } => {
                assert!(reason.to_ascii_lowercase().contains("lotro") || reason.contains("Rings"));
            }
            other => panic!("{name} should be excluded, got {other:?}"),
        }
    }
}

#[test]
fn ark_and_eso_are_not_default_excluded() {
    assert_eq!(
        decide(PolicyInput::new("ARK: Survival Evolved")),
        CompactPolicy::Lzx
    );
    assert_eq!(
        decide(PolicyInput::new("The Elder Scrolls Online")),
        CompactPolicy::Lzx
    );
    assert_eq!(decide(PolicyInput::new("Warframe")), CompactPolicy::Lzx);
}

#[test]
fn gw2_folder_marker_excludes() {
    let mut input = PolicyInput::new("Some Launcher Shortcut");
    input.install_folder = Some(Path::new(r"D:\Games\Guild Wars 2"));
    match decide(input) {
        CompactPolicy::Exclude { .. } => {}
        other => panic!("folder marker should exclude, got {other:?}"),
    }
}

#[test]
fn gw2_steam_id_excludes() {
    let mut input = PolicyInput::new("Unknown");
    input.store_id = Some("steam");
    input.launcher_id = Some("1284210");
    match decide(input) {
        CompactPolicy::Exclude { .. } => {}
        other => panic!("steam id should exclude, got {other:?}"),
    }
}

#[test]
fn unknown_title_is_not_excluded() {
    assert_eq!(decide(PolicyInput::new("Hades")), CompactPolicy::Lzx);
    assert_eq!(decide(PolicyInput::new("Celeste")), CompactPolicy::Lzx);
    assert_eq!(decide(PolicyInput::new("Dishonored 2")), CompactPolicy::Lzx);
}

#[test]
fn excluded_never_recommends_any_compact() {
    let policy = decide(PolicyInput::new("Guild Wars 2"));
    assert_eq!(policy.as_compact_exe_algo(), None);
}

#[test]
fn denylist_is_extendable() {
    let mut list = DenyList::empty();
    list.extend_with(DenyRule {
        reason: "test custom rewriter".into(),
        names: vec!["Custom Rewriter".into()],
        ids: vec![],
        folder_markers: vec![],
    });
    let mut input = PolicyInput::new("custom rewriter");
    let policy = recommend(&input, now(), &ShelfConfig::default(), &list);
    assert!(matches!(policy, CompactPolicy::Exclude { .. }));
    input.title = "Hades";
    assert_eq!(
        recommend(&input, now(), &ShelfConfig::default(), &list),
        CompactPolicy::Lzx
    );
}
