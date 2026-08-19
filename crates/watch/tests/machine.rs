//! State-machine table tests for Live Compact.
//!
//! idle → downloading → lock;
//! still downloading → stay locked;
//! download complete → incremental recompact of only new/uncompressed;
//! already-compacted files not submitted;
//! no `/F`;
//! no FS-watcher subscription during patch.

use std::path::PathBuf;

use watch::{
    CompactEvent, FileCompactState, IncrementalPlan, InstallFile, LiveWatch, MemoryInventory,
    MemorySteam, RecordingCompactor, TickEvent, TitleStatus, DOWNLOADING, FULLY_INSTALLED,
    UPDATE_REQUIRED, UPDATE_STARTED,
};

fn title(flags: u32) -> TitleStatus {
    TitleStatus {
        app_id: 570,
        name: "Dota 2".into(),
        install_dir: PathBuf::from("dota 2 beta"),
        state_flags: flags,
        bytes_to_download: 0,
        bytes_downloaded: 0,
        steam_downloading: false,
    }
}

fn inventory_mixed() -> MemoryInventory {
    let mut inv = MemoryInventory::default();
    inv.files.insert(
        "570".into(),
        vec![
            InstallFile {
                relative_path: PathBuf::from("game.exe"),
                compact: FileCompactState::Compressed,
                appeared_after_lock: false,
            },
            InstallFile {
                relative_path: PathBuf::from("old_data.vpk"),
                compact: FileCompactState::Uncompressed,
                appeared_after_lock: false,
            },
            InstallFile {
                relative_path: PathBuf::from("new_patch.vpk"),
                compact: FileCompactState::Uncompressed,
                appeared_after_lock: true,
            },
            InstallFile {
                relative_path: PathBuf::from("already.lzx"),
                compact: FileCompactState::Compressed,
                appeared_after_lock: false,
            },
        ],
    );
    inv
}

struct Case {
    name: &'static str,
    start_flags: u32,
    start_probe: bool,
    next_flags: u32,
    next_probe: bool,
    expect_first: &'static [fn(&TickEvent) -> bool],
    expect_second_has_lock_stay: bool,
    expect_incremental: bool,
}

fn is_locked(e: &TickEvent) -> bool {
    matches!(e, TickEvent::Locked { title_id } if title_id == "570")
}
fn is_stayed(e: &TickEvent) -> bool {
    matches!(e, TickEvent::StayedLocked { title_id } if title_id == "570")
}

#[test]
fn table_idle_download_lock_stay_complete_incremental() {
    let cases = [
        Case {
            name: "idle -> downloading bit -> lock",
            start_flags: FULLY_INSTALLED,
            start_probe: false,
            next_flags: FULLY_INSTALLED | DOWNLOADING,
            next_probe: false,
            expect_first: &[],
            expect_second_has_lock_stay: true,
            expect_incremental: false,
        },
        Case {
            name: "1026 update started locks",
            start_flags: UPDATE_REQUIRED | UPDATE_STARTED,
            start_probe: false,
            next_flags: UPDATE_REQUIRED | UPDATE_STARTED,
            next_probe: false,
            expect_first: &[is_locked],
            expect_second_has_lock_stay: true,
            expect_incremental: false,
        },
    ];

    for case in cases {
        let steam = MemorySteam::new(vec![TitleStatus {
            steam_downloading: case.start_probe,
            ..title(case.start_flags)
        }]);
        let mut watch = LiveWatch::new(steam, RecordingCompactor::default(), inventory_mixed());
        let first = watch.tick().unwrap();
        for pred in case.expect_first {
            assert!(
                first.iter().any(pred),
                "{}: first tick events {first:?}",
                case.name
            );
        }
        watch.status.titles[0].state_flags = case.next_flags;
        watch.status.titles[0].steam_downloading = case.next_probe;
        let second = watch.tick().unwrap();
        if case.expect_second_has_lock_stay {
            assert!(
                second.iter().any(|e| is_locked(e) || is_stayed(e)),
                "{}: second tick {second:?}",
                case.name
            );
        }
        if !case.expect_incremental {
            assert!(
                !second
                    .iter()
                    .any(|e| matches!(e, TickEvent::IncrementalRecompact { .. })),
                "{}: unexpected incremental {second:?}",
                case.name
            );
        }
        assert!(
            !watch.fs_watch.is_subscribed(),
            "{}: FS watch must not be subscribed during patch",
            case.name
        );
    }
}

#[test]
fn idle_then_downloading_locks_then_stays_then_incrementally_recompacts() {
    let steam = MemorySteam::new(vec![title(FULLY_INSTALLED)]);
    let mut watch = LiveWatch::new(steam, RecordingCompactor::default(), inventory_mixed());

    let idle = watch.tick().unwrap();
    assert!(idle.is_empty(), "idle installed must not lock: {idle:?}");
    assert!(!watch.is_locked("570"));
    assert!(!watch
        .compact
        .events
        .iter()
        .any(|e| matches!(e, CompactEvent::Lock(_))));

    watch.status.titles[0].state_flags = FULLY_INSTALLED | DOWNLOADING;
    watch.status.titles[0].bytes_to_download = 1_000;
    watch.status.titles[0].bytes_downloaded = 10;
    watch.status.titles[0].steam_downloading = true;
    let locked = watch.tick().unwrap();
    assert!(locked.iter().any(is_locked), "{locked:?}");
    assert!(watch.is_locked("570"));
    assert!(
        !watch.fs_watch.is_subscribed(),
        "no FS-watcher subscription during patch"
    );
    watch.fs_watch.subscribe_idle_only(true);
    assert!(
        !watch.fs_watch.is_subscribed(),
        "subscribe_idle_only must refuse mid-patch"
    );

    let stayed = watch.tick().unwrap();
    assert!(stayed.iter().any(is_stayed), "{stayed:?}");
    assert_eq!(watch.compact.locks(), vec!["570"]);

    watch.status.titles[0].state_flags = FULLY_INSTALLED;
    watch.status.titles[0].bytes_to_download = 0;
    watch.status.titles[0].bytes_downloaded = 0;
    watch.status.titles[0].steam_downloading = false;
    let done = watch.tick().unwrap();
    assert!(
        done.iter()
            .any(|e| matches!(e, TickEvent::IncrementalRecompact { files: 2, .. })),
        "only new + uncompressed (2 files): {done:?}"
    );
    assert!(done.iter().any(|e| matches!(e, TickEvent::Unlocked { .. })));
    assert!(!watch.is_locked("570"));

    let incrementals = watch.compact.incrementals();
    assert_eq!(incrementals.len(), 1);
    match &incrementals[0] {
        CompactEvent::Incremental { files, .. } => {
            assert!(files.contains(&PathBuf::from("old_data.vpk")));
            assert!(files.contains(&PathBuf::from("new_patch.vpk")));
            assert!(!files.contains(&PathBuf::from("game.exe")));
            assert!(!files.contains(&PathBuf::from("already.lzx")));
            assert!(!files.iter().any(|p| p.to_string_lossy().contains("/F")));
        }
        other => panic!("expected incremental, got {other:?}"),
    }
    assert!(watch.compact.events.iter().all(|e| !e.is_force_full_tree()));
}

#[test]
fn already_compacted_only_does_not_submit_incremental() {
    let mut inv = MemoryInventory::default();
    inv.files.insert(
        "570".into(),
        vec![InstallFile {
            relative_path: PathBuf::from("packed.lzx"),
            compact: FileCompactState::Compressed,
            appeared_after_lock: false,
        }],
    );
    let steam = MemorySteam::new(vec![title(FULLY_INSTALLED | DOWNLOADING)]);
    let mut watch = LiveWatch::new(steam, RecordingCompactor::default(), inv);
    watch.tick().unwrap();
    watch.status.titles[0].state_flags = FULLY_INSTALLED;
    let done = watch.tick().unwrap();
    assert!(
        !done
            .iter()
            .any(|e| matches!(e, TickEvent::IncrementalRecompact { .. })),
        "{done:?}"
    );
    assert!(watch.compact.incrementals().is_empty());
    assert!(watch
        .compact
        .events
        .iter()
        .any(|e| matches!(e, CompactEvent::Unlock(_))));
}

#[test]
fn incremental_plan_never_names_the_whole_tree() {
    let files = vec![
        InstallFile {
            relative_path: PathBuf::from("a.bin"),
            compact: FileCompactState::Uncompressed,
            appeared_after_lock: false,
        },
        InstallFile {
            relative_path: PathBuf::from("b.bin"),
            compact: FileCompactState::Compressed,
            appeared_after_lock: false,
        },
    ];
    let plan = IncrementalPlan::from_inventory("570", &files);
    assert_eq!(plan.files, vec![PathBuf::from("a.bin")]);
}

#[test]
fn acf_text_round_trip_into_status() {
    let text = r#"
"AppState"
{
	"appid"		"570"
	"name"		"Dota 2"
	"StateFlags"		"4"
	"installdir"		"dota 2 beta"
	"BytesToDownload"		"0"
	"BytesDownloaded"		"0"
}
"#;
    let t = watch::title_from_acf_text(std::path::Path::new("appmanifest_570.acf"), text, false)
        .unwrap();
    assert_eq!(t.app_id, 570);
    assert_eq!(t.state_flags, FULLY_INSTALLED);
    assert!(!t.is_patching());
}
