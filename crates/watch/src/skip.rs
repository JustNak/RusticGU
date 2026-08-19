//! Compact-candidate skip lists (extensions + folders).
//!
//! These are **data**, not scattered one-off string matches. Incremental
//! recompact uses [`is_compact_candidate`] so already-compressed media/archives
//! and GPU shader caches are never submitted to WOF.
//!
//! Do **not** hard-skip: `wav`, `dds`, `bnk`.
//! Do **not** skip container/game-data formats: `vpk`, `vpak`, `pak`, `cpk`,
//! `arc`, `dat`, `bin`, `uasset`, `uexp`, `ubulk`.

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

/// True when any path segment is a GPU/pipeline cache folder, including
/// `Saved\PipelineCaches`.
pub fn folder_is_skipped(path: &Path) -> bool {
    if is_steam_shadercache_path(path) {
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
}
