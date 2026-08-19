//! Compact-candidate skip lists (extensions + folders).
//!
//! These are **data**, not scattered one-off string matches. Incremental
//! recompact uses [`is_compact_candidate`] so already-compressed media/archives,
//! GPU shader caches, and **in-tree save folders** are never submitted to WOF.
//!
//! Do **not** hard-skip: `wav`, `dds`, `bnk`.
//! Do **not** skip container/game-data formats: `vpk`, `vpak`, `pak`, `cpk`,
//! `arc`, `dat`, `bin`, `uasset`, `uexp`, `ubulk` — unless the path sits under
//! a save folder ([`SKIP_SAVE_FOLDER_SEGMENTS`]).
//!
//! Do **not** skip every Unreal `Saved` tree; only `Saved\SaveGames` /
//! `Saved\Save` (plus explicit save segment names).

use std::path::Path;

/// Already-compressed / not-worth-WOF extensions (no leading dot).
pub const SKIP_EXTENSIONS: &[&str] = &[
    // video
    "bik", "bk2", "bik2", "pc_binkvid", "mp4", "webm", "mkv", "avi", "wmv", "flv", "mpg", "m2v",
    "m4v", "vob", "usm", "ivf",
    // audio
    "mp3", "ogg", "wma", "flac", "opus", "m4a", "aac", "wem", "fsb", "xwma",
    // textures / images
    "jpg", "jpeg", "png", "webp", "ktx", "ktx2", "basis", "basisu", "astc", "pvr", "crn", "tfc",
    // archives
    "zip", "7z", "rar", "gz", "xz", "cab", "bz2", "tgz", "lz", "txz", "dmg", "lzx", "br", "lz4",
    "lzma", "zst", "zstd",
    // junk
    "log", "dmp", "tmp",
];

/// Explicitly compact-eligible extensions (must never appear in [`SKIP_EXTENSIONS`]).
pub const ELIGIBLE_EXTENSIONS: &[&str] = &[
    "wav", "dds", "bnk", "vpk", "vpak", "pak", "cpk", "arc", "dat", "bin", "uasset", "uexp", "ubulk",
];

/// Folder segment names that are GPU / pipeline caches, not game files.
pub const SKIP_FOLDER_SEGMENTS: &[&str] = &[
    "ShaderCache",
    "shadercache",
    "PipelineCache",
    "PipelineCaches",
    "PSOCache",
    "DXCache",
    "VkCache",
    "GLCache",
];

/// In-tree save directory segments (case-insensitive).
///
/// `dat` / `bin` stay extension-eligible **unless** a path contains one of
/// these. `Saved` alone is **not** listed — Unreal `Saved\Config` / `Logs` /
/// `Paks` must remain compact-eligible. `Saved\SaveGames` and `Saved\Save`
/// are matched as a two-segment pair in [`folder_is_skipped`].
pub const SKIP_SAVE_FOLDER_SEGMENTS: &[&str] = &[
    "SaveGames",
    "saves",
    "Saved Games",
    "Save",
    "save",
    "SaveGame",
    "savegame",
    "SaveData",
    "savedata",
    "UserSaves",
];

fn path_segments(path: &Path) -> Vec<String> {
    path.to_string_lossy()
        .split(['/', '\\'])
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn extension_of(path: &Path) -> Option<String> {
    let name = path
        .file_name()
        .or_else(|| path.components().last().map(|c| c.as_os_str()))
        .and_then(|n| n.to_str())?;
    // Handle `D:\foo\bar.mp4` on Linux (whole path is one component).
    let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let ext = name.rsplit_once('.')?.1;
    if ext.is_empty() {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

/// True when the file extension is on the WOF skip list.
pub fn extension_is_skipped(path: &Path) -> bool {
    match extension_of(path) {
        Some(ext) => SKIP_EXTENSIONS.iter().any(|s| s.eq_ignore_ascii_case(&ext)),
        None => false,
    }
}

/// Steam shadercache lives at `{library}\steamapps\shadercache\{appid}\`,
/// **outside** `steamapps\common\`. That tree is never a game-file compact target.
pub fn is_steam_shadercache_path(path: &Path) -> bool {
    let segs: Vec<String> = path_segments(path)
        .into_iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    segs.windows(2)
        .any(|w| w[0] == "steamapps" && w[1] == "shadercache")
}

/// True when the path sits under a save directory (`SaveGames`, `saves`,
/// `Saved Games`, `Save` / `SaveGame` / `SaveData` / `UserSaves`, or Unreal
/// `Saved\SaveGames` / `Saved\Save`).
pub fn is_save_folder_path(path: &Path) -> bool {
    let segs = path_segments(path);
    if segs.windows(2).any(|w| {
        w[0].eq_ignore_ascii_case("Saved")
            && (w[1].eq_ignore_ascii_case("SaveGames") || w[1].eq_ignore_ascii_case("Save"))
    }) {
        return true;
    }
    segs.iter().any(|seg| {
        SKIP_SAVE_FOLDER_SEGMENTS
            .iter()
            .any(|skip| seg.eq_ignore_ascii_case(skip))
    })
}

/// True when any path segment is a GPU/pipeline cache folder, a save
/// folder, `Saved\PipelineCaches`, or Steam `steamapps\shadercache`.
pub fn folder_is_skipped(path: &Path) -> bool {
    if is_steam_shadercache_path(path) || is_save_folder_path(path) {
        return true;
    }
    let segs = path_segments(path);
    if segs.windows(2).any(|w| {
        w[0].eq_ignore_ascii_case("Saved") && w[1].eq_ignore_ascii_case("PipelineCaches")
    }) {
        return true;
    }
    segs.iter().any(|seg| {
        SKIP_FOLDER_SEGMENTS
            .iter()
            .any(|skip| seg.eq_ignore_ascii_case(skip))
    })
}

/// Whether incremental recompact / compact may consider this path.
pub fn is_compact_candidate(path: &Path) -> bool {
    !extension_is_skipped(path) && !folder_is_skipped(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn eligible_exts_are_not_on_skip_list() {
        for ext in ELIGIBLE_EXTENSIONS {
            assert!(
                !SKIP_EXTENSIONS.iter().any(|s| s.eq_ignore_ascii_case(ext)),
                "{ext} must remain compact-eligible"
            );
        }
    }

    #[test]
    fn wav_dds_bnk_and_containers_are_eligible() {
        for name in [
            "sound.wav",
            "albedo.dds",
            "events.bnk",
            "pak/game.vpk",
            "a.vpak",
            "data.pak",
            "arc.cpk",
            "bundle.arc",
            "save.dat",
            "blob.bin",
            "Foo.uasset",
            "Foo.uexp",
            "Foo.ubulk",
        ] {
            assert!(
                is_compact_candidate(Path::new(name)),
                "{name} should be eligible"
            );
        }
    }

    #[test]
    fn media_archives_logs_are_skipped() {
        for name in ["cutscene.mp4", "voice.wem", "tex.ktx2", "pack.zst", "debug.log"] {
            assert!(
                extension_is_skipped(Path::new(name)),
                "{name} should be skipped"
            );
            assert!(!is_compact_candidate(Path::new(name)));
        }
    }

    #[test]
    fn shader_and_pipeline_folders_skipped() {
        assert!(folder_is_skipped(Path::new(
            r"C:\Games\Title\ShaderCache\ps.cache"
        )));
        assert!(folder_is_skipped(Path::new(
            r"C:\Games\Title\Saved\PipelineCaches\pso"
        )));
        assert!(folder_is_skipped(Path::new(
            r"E:\SteamLibrary\steamapps\shadercache\570\foo"
        )));
        assert!(!folder_is_skipped(Path::new(
            r"E:\SteamLibrary\steamapps\common\Dota 2\game\dota\pak01.vpk"
        )));
    }

    #[test]
    fn save_folders_skipped_but_not_whole_saved_tree() {
        assert!(is_save_folder_path(Path::new(r"game\SaveGames\foo.dat")));
        assert!(is_save_folder_path(Path::new(r"game\saves\slot.bin")));
        assert!(is_save_folder_path(Path::new(
            r"My Game\Saved Games\profile.sav"
        )));
        assert!(is_save_folder_path(Path::new(
            r"game\Saved\SaveGames\slot.sav"
        )));
        assert!(!is_save_folder_path(Path::new(r"game\Saved\Config\Game.ini")));
        assert!(!is_save_folder_path(Path::new(r"game\data\foo.dat")));
        assert!(is_compact_candidate(Path::new(r"game\data\foo.dat")));
        assert!(is_compact_candidate(Path::new(r"game\Saved\Paks\pak.bin")));
        assert!(!is_compact_candidate(Path::new(r"game\SaveGames\foo.dat")));
        assert!(!is_compact_candidate(Path::new(r"game\saves\slot.bin")));
    }
}
