//! Skip-ext and folder-skip for incremental recompact candidates.

use std::path::PathBuf;

use watch::{
    is_compact_candidate, FileCompactState, IncrementalPlan, InstallFile, ELIGIBLE_EXTENSIONS,
};

fn file(path: &str, uncompressed: bool) -> InstallFile {
    InstallFile {
        relative_path: PathBuf::from(path),
        compact: if uncompressed {
            FileCompactState::Uncompressed
        } else {
            FileCompactState::Compressed
        },
        appeared_after_lock: uncompressed,
    }
}

#[test]
fn wav_dds_bnk_and_containers_eligible_media_skipped() {
    for ext in ELIGIBLE_EXTENSIONS {
        assert!(
            is_compact_candidate(&PathBuf::from(format!("asset.{ext}"))),
            "{ext}"
        );
    }
    let plan = IncrementalPlan::from_inventory(
        "570",
        &[
            file("voice.wav", true),
            file("albedo.dds", true),
            file("events.bnk", true),
            file("dir/game.vpk", true),
            file("data.pak", true),
            file("Foo.uasset", true),
            file("cutscene.mp4", true),
            file("videos_assets_all_2ab8.bundle", true),
            file("music_assets_musictitle.bundle", true),
            file("asset_references_assets_all.bundle", true),
            file("voice.wem", true),
            file("tex.ktx2", true),
            file("pack.zst", true),
            file("debug.log", true),
        ],
    );
    let names: Vec<_> = plan
        .files
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().any(|n| n.ends_with(".wav")));
    assert!(names.iter().any(|n| n.ends_with(".dds")));
    assert!(names.iter().any(|n| n.ends_with(".bnk")));
    assert!(names.iter().any(|n| n.ends_with(".vpk")));
    assert!(names.iter().any(|n| n.ends_with(".pak")));
    assert!(names.iter().any(|n| n.ends_with(".uasset")));
    assert!(!names.iter().any(|n| n.ends_with(".mp4")));
    assert!(!names.iter().any(|n| n.contains("videos_")));
    assert!(!names.iter().any(|n| n.contains("music_assets")));
    assert!(names
        .iter()
        .any(|n| n.ends_with("asset_references_assets_all.bundle")));
    assert!(!names.iter().any(|n| n.ends_with(".wem")));
    assert!(!names.iter().any(|n| n.ends_with(".ktx2")));
    assert!(!names.iter().any(|n| n.ends_with(".zst")));
    assert!(!names.iter().any(|n| n.ends_with(".log")));
}

#[test]
fn save_folders_skip_dat_bin_inside_but_not_outside() {
    assert!(is_compact_candidate(&PathBuf::from(r"game\data\foo.dat")));
    assert!(is_compact_candidate(&PathBuf::from(r"game\data\slot.bin")));
    assert!(!is_compact_candidate(&PathBuf::from(
        r"game\SaveGames\foo.dat"
    )));
    assert!(!is_compact_candidate(&PathBuf::from(
        r"game\saves\slot.bin"
    )));
    assert!(!is_compact_candidate(&PathBuf::from(
        r"My Game\Saved Games\profile.sav"
    )));

    let plan = IncrementalPlan::from_inventory(
        "570",
        &[
            file(r"game\data\foo.dat", true),
            file(r"game\SaveGames\foo.dat", true),
            file(r"game\saves\slot.bin", true),
            file(r"My Game\Saved Games\profile.sav", true),
            file(r"game\Saved\SaveGames\ue.sav", true),
            file(r"game\Saved\Config\Game.ini", true),
        ],
    );
    let names: Vec<_> = plan
        .files
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    assert!(
        names.iter().any(|n| n.ends_with(r"data\foo.dat")),
        "{names:?}"
    );
    assert!(names.iter().any(|n| n.contains("Config")), "{names:?}");
    assert!(!names.iter().any(|n| n.contains("SaveGames")));
    assert!(!names.iter().any(|n| n.contains("saves")));
    assert!(!names.iter().any(|n| n.contains("Saved Games")));
}

#[test]
fn shadercache_and_pipeline_folders_skipped() {
    let plan = IncrementalPlan::from_inventory(
        "570",
        &[
            file(r"game\pak01.vpk", true),
            file(r"ShaderCache\ps.bin", true),
            file(r"Saved\PipelineCaches\pso", true),
            file(r"E:\SteamLibrary\steamapps\shadercache\570\c.cache", true),
        ],
    );
    assert_eq!(plan.files, vec![PathBuf::from(r"game\pak01.vpk")]);
}
