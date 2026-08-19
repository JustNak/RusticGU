//! Fixture-driven discovery: skip-if-absent, dual-register, Xbox opt-in,
//! and proof that itch `butler.db` is never opened.

use std::path::{Path, PathBuf};

use stores::{
    discover_all, DiscoverOptions, IndexFs, MemoryHive, PathRoots, RecordingFs, StdFs, StoreId,
    StoreProbe,
};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn full_roots() -> PathRoots {
    let fx = fixtures();
    PathRoots {
        epic_manifests: Some(fx.join("epic/Manifests")),
        origin_local_content: Some(fx.join("origin/LocalContent")),
        ea_install_index: Some(fx.join("ea/install_index.json")),
        ubisoft_index: Some(fx.join("ubisoft/games.json")),
        riot_metadata: Some(fx.join("riot/Metadata")),
        riot_installs: Some(fx.join("riot/RiotClientInstalls.json")),
        battlenet_agent: Some(fx.join("battlenet/Agent")),
        itch_config: Some(fx.join("itch")),
        xbox_games_roots: vec![fx.join("xbox/XboxGames")],
    }
}

fn gog_and_ubi_hive() -> MemoryHive {
    let mut hive = MemoryHive::new();
    hive.set_value(
        r"SOFTWARE\WOW6432Node\GOG.com\Games\1207659012",
        "PATH",
        r"D:\GOG\Hades",
    );
    hive.set_value(
        r"SOFTWARE\WOW6432Node\GOG.com\Games\1207659012",
        "gameName",
        "Hades",
    );
    hive.set_value(
        r"SOFTWARE\WOW6432Node\GOG.com\Games\1207659012",
        "gameID",
        "1207659012",
    );
    // Same game mirrored in the non-WOW hive must not duplicate within GOG.
    hive.set_value(
        r"SOFTWARE\GOG.com\Games\1207659012",
        "path",
        r"D:\GOG\Hades",
    );
    hive.set_value(r"SOFTWARE\GOG.com\Games\1207659012", "GAMENAME", "Hades");
    hive.set_value(r"SOFTWARE\GOG.com\Games\1207659012", "gameID", "1207659012");

    hive.set_value(
        r"SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs\80",
        "InstallDir",
        r"C:\Ubisoft\Rayman Origins",
    );
    hive
}

#[test]
fn skip_if_absent_is_empty_not_an_error() {
    let probe = StoreProbe::new(StdFs, MemoryHive::new(), PathRoots::default());
    let report = probe.discover_all(&DiscoverOptions::default());
    assert!(report.titles.is_empty());
    assert!(
        report
            .warnings
            .iter()
            .all(|w| w.store != StoreId::XboxGames || w.message.contains("opt-in")),
        "absent launchers must not warn as hard failures: {:?}",
        report.warnings
    );
}

#[test]
fn epic_parses_manifests_and_skips_incomplete() {
    let report = discover_all(
        &StdFs,
        &MemoryHive::new(),
        &full_roots(),
        &DiscoverOptions::default(),
    );
    let epic: Vec<_> = report.titles_for(StoreId::Epic).collect();
    assert!(epic.iter().any(|t| t.title == "Hades"));
    assert!(epic.iter().any(|t| t.title == "Fortnite"));
    assert!(epic.iter().all(|t| t.title != "Unfinished Download"));
}

#[test]
fn dual_register_same_title_from_two_stores() {
    let report = discover_all(
        &StdFs,
        &gog_and_ubi_hive(),
        &full_roots(),
        &DiscoverOptions::default(),
    );
    let hades: Vec<_> = report
        .titles
        .iter()
        .filter(|t| t.title.eq_ignore_ascii_case("Hades"))
        .collect();
    assert_eq!(hades.len(), 2, "Epic + GOG Hades must both appear: {hades:?}");
    let stores: Vec<StoreId> = hades.iter().map(|t| t.store).collect();
    assert!(stores.contains(&StoreId::Epic));
    assert!(stores.contains(&StoreId::Gog));
}

#[test]
fn gog_reads_path_name_id_from_fake_hive() {
    let report = discover_all(
        &StdFs,
        &gog_and_ubi_hive(),
        &PathRoots::default(),
        &DiscoverOptions::default(),
    );
    let gog: Vec<_> = report.titles_for(StoreId::Gog).collect();
    assert_eq!(gog.len(), 1);
    assert_eq!(gog[0].title, "Hades");
    assert_eq!(gog[0].launcher_id.as_deref(), Some("1207659012"));
    assert_eq!(gog[0].install_path, PathBuf::from(r"D:\GOG\Hades"));
}

#[test]
fn ea_origin_and_desktop_index() {
    let report = discover_all(
        &StdFs,
        &MemoryHive::new(),
        &full_roots(),
        &DiscoverOptions::default(),
    );
    let ea: Vec<_> = report.titles_for(StoreId::Ea).map(|t| t.title.as_str()).collect();
    assert!(ea.contains(&"Titanfall 2"), "{ea:?}");
    assert!(ea.contains(&"Apex Legends"), "{ea:?}");
}

#[test]
fn ubisoft_registry_and_json_index() {
    let report = discover_all(
        &StdFs,
        &gog_and_ubi_hive(),
        &full_roots(),
        &DiscoverOptions::default(),
    );
    let ubi: Vec<_> = report.titles_for(StoreId::Ubisoft).collect();
    assert!(ubi.iter().any(|t| t.launcher_id.as_deref() == Some("80")));
    assert!(ubi.iter().any(|t| t.title.contains("Valhalla")));
}

#[test]
fn riot_reads_installed_yaml_skips_uninstalled_leftover() {
    let report = discover_all(
        &StdFs,
        &MemoryHive::new(),
        &full_roots(),
        &DiscoverOptions::default(),
    );
    let riot: Vec<_> = report.titles_for(StoreId::Riot).collect();
    assert!(
        riot.iter().any(|t| t.launcher_id.as_deref() == Some("valorant.live")),
        "{riot:?}"
    );
    assert!(
        riot.iter()
            .all(|t| t.launcher_id.as_deref() != Some("league_of_legends.live")),
        "uninstalled leftover must be skipped: {riot:?}"
    );
}

#[test]
fn battlenet_skips_agent_product() {
    let report = discover_all(
        &StdFs,
        &MemoryHive::new(),
        &full_roots(),
        &DiscoverOptions::default(),
    );
    let bn: Vec<_> = report.titles_for(StoreId::Battlenet).collect();
    assert_eq!(bn.len(), 1, "{bn:?}");
    assert_eq!(bn[0].title, "World of Warcraft");
    assert_eq!(bn[0].launcher_id.as_deref(), Some("wow"));
}

#[test]
fn itch_uses_library_index_and_never_opens_butler_db() {
    let fx = fixtures();
    let rec = RecordingFs::new(StdFs);
    let roots = full_roots();
    let report = StoreProbe::new(&rec, MemoryHive::new(), roots).discover_all(&DiscoverOptions::default());
    let itch: Vec<_> = report.titles_for(StoreId::Itch).collect();
    assert!(itch.iter().any(|t| t.title == "Celeste"), "{itch:?}");

    let opened = rec.opened_paths();
    assert!(
        stores::fs::never_opened_butler_db(&opened),
        "butler.db must never be opened, got {opened:?}"
    );
    assert!(
        fx.join("itch/butler.db").is_file(),
        "fixture must actually contain butler.db so the test is meaningful"
    );
}

#[test]
fn xbox_default_off_vs_opt_in() {
    let roots = full_roots();
    let off = discover_all(&StdFs, &MemoryHive::new(), &roots, &DiscoverOptions::default());
    assert!(
        off.titles_for(StoreId::XboxGames).next().is_none(),
        "XboxGames must be off by default"
    );

    let on = discover_all(
        &StdFs,
        &MemoryHive::new(),
        &roots,
        &DiscoverOptions {
            include_xbox_games: true,
        },
    );
    let xbox: Vec<_> = on.titles_for(StoreId::XboxGames).collect();
    assert_eq!(xbox.len(), 1, "{xbox:?}");
    assert_eq!(xbox[0].title, "Forza Horizon 5");
}

#[test]
fn volume_root_is_refused() {
    let err = stores::fs::reject_forbidden(Path::new("D:\\")).unwrap_err();
    assert!(err.to_string().contains("volume root"));
    let err = stores::fs::StdFs
        .read_to_string(Path::new("D:\\"))
        .unwrap_err();
    assert!(matches!(err, stores::StoreError::Forbidden { .. }));
}

#[test]
fn butler_db_read_is_forbidden_even_if_asked() {
    let path = fixtures().join("itch/butler.db");
    let err = StdFs.read_to_string(&path).unwrap_err();
    assert!(matches!(err, stores::StoreError::Forbidden { .. }));
}
