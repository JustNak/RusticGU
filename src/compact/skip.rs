//! Files and folders the compact engine must leave alone.

use std::path::Path;

/// Video / audio / image / archive / log extensions that must not be WOF-compressed.
///
/// `wav`, `dds`, and `bnk` are intentionally **not** on this list.
const SKIP_EXTENSIONS: &[&str] = &[
    // video
    "bik",
    "bk2",
    "bik2",
    "pc_binkvid",
    "mp4",
    "webm",
    "mkv",
    "avi",
    "wmv",
    "flv",
    "mpg",
    "m2v",
    "m4v",
    "vob",
    "usm",
    "ivf", // audio (not wav)
    "mp3",
    "ogg",
    "wma",
    "flac",
    "opus",
    "m4a",
    "aac",
    "wem",
    "fsb",
    "xwma",
    // images / textures (not dds)
    "jpg",
    "jpeg",
    "png",
    "webp",
    "ktx",
    "ktx2",
    "basis",
    "basisu",
    "astc",
    "pvr",
    "crn",
    "tfc",
    // archives
    "zip",
    "7z",
    "rar",
    "gz",
    "xz",
    "cab",
    "bz2",
    "tgz",
    "lz",
    "txz",
    "dmg",
    "lzx",
    "br",
    "lz4",
    "lzma",
    "zst",
    "zstd", // logs / temp
    "log",
    "dmp",
    "tmp",
];

/// Directory names (case-insensitive) to skip entirely when they appear in the tree.
const SKIP_DIR_NAMES: &[&str] = &[
    "savegames",
    "saved games",
    "saves",
    "shadercache",
    "shader cache",
    "pipelinecache",
    "pipelinecaches",
    "psocache",
    "dxcache",
    "vkcache",
    "glcache",
    "nv_cache",
    "gpucache",
    "logs",
    "log",
    "dumps",
    "crashdumps",
    "crash_dumps",
];

/// Titles that must not be offered for compact (name or install-folder match).
const AUTO_EXCLUDE_TITLES: &[&str] = &["guild wars 2", "secret world legends"];

const DSTORAGE_FILENAMES: &[&str] = &["dstorage.dll", "dstoragecore.dll"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    Extension,
    Directory,
    LogDumpName,
    /// Unity/addressable container whose name is packed video/audio, or a file
    /// whose header is already a compressed media bitstream.
    PackedMedia,
}

/// Unity / addressable containers that can wrap already-compressed media.
const MEDIA_CONTAINER_EXTS: &[&str] = &["bundle", "assets", "resource", "ress"];

/// Filename tokens (split on non-alphanumerics) that mean packed video/audio.
///
/// `videos_assets_all_*.bundle` matches; `asset_references_*.bundle` does not.
const PACKED_MEDIA_TOKENS: &[&str] = &[
    "video",
    "videos",
    "movie",
    "movies",
    "cutscene",
    "cutscenes",
    "music",
    "audio",
    "sound",
    "sounds",
    "jingle",
    "soundtrack",
];

/// True when this path should not be passed to `compact /EXE`.
pub fn should_skip(path: &Path) -> bool {
    skip_reason(path).is_some()
}

pub fn skip_reason(path: &Path) -> Option<SkipReason> {
    if path_has_skipped_dir(path) {
        return Some(SkipReason::Directory);
    }
    if file_looks_like_log_or_dump(path) {
        return Some(SkipReason::LogDumpName);
    }
    if extension_is_skipped(path) {
        return Some(SkipReason::Extension);
    }
    if packed_media_container(path) || file_has_incompressible_media_magic(path) {
        return Some(SkipReason::PackedMedia);
    }
    None
}

/// True when this is a game-data container named as packed video/audio.
pub fn packed_media_container(path: &Path) -> bool {
    match extension_lower(path) {
        Some(ext) if MEDIA_CONTAINER_EXTS.iter().any(|s| *s == ext) => {
            filename_has_packed_media_token(path)
        }
        _ => false,
    }
}

fn filename_has_packed_media_token(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .any(|token| PACKED_MEDIA_TOKENS.contains(&token))
}

/// Peek the header of an on-disk file for already-compressed media bitstreams.
///
/// Missing files (unit-test path stubs) are not skipped. WAVE/RIFF is left
/// alone so `.wav` without an extension stays eligible.
fn file_has_incompressible_media_magic(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = [0u8; 16];
    let Ok(n) = file.read(&mut buf) else {
        return false;
    };
    let b = &buf[..n];
    if b.len() >= 8 && &b[4..8] == b"ftyp" {
        return true;
    }
    if b.starts_with(b"OggS")
        || b.starts_with(b"ID3")
        || b.starts_with(b"FSB5")
        || b.starts_with(b"FSB4")
        || b.starts_with(b"BIK")
        || b.starts_with(b"KB2")
    {
        return true;
    }
    if b.len() >= 2 && b[0] == 0xFF && matches!(b[1], 0xFB | 0xF3 | 0xF2) {
        return true;
    }
    b.len() >= 4 && b[0] == 0x1A && b[1] == 0x45 && b[2] == 0xDF && b[3] == 0xA3
}

pub fn extension_is_skipped(path: &Path) -> bool {
    let ext = extension_lower(path);
    match ext {
        Some(ext) => SKIP_EXTENSIONS.iter().any(|s| *s == ext),
        None => false,
    }
}

fn extension_lower(path: &Path) -> Option<String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .or_else(|| path.to_str())
        .unwrap_or("")
        .replace('\\', "/");
    let name = name.rsplit('/').next().unwrap_or(name.as_str());
    let ext = name.rsplit_once('.')?.1;
    Some(ext.trim().to_ascii_lowercase())
}

pub fn path_has_skipped_dir(path: &Path) -> bool {
    // Split on both separators so Windows-style fixtures still match on Linux CI hosts.
    let raw = path.to_string_lossy().replace('\\', "/");
    let parts: Vec<String> = raw
        .split('/')
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_lowercase())
        .collect();
    if parts
        .windows(2)
        .any(|pair| pair[0] == "saved" && pair[1] == "pipelinecaches")
    {
        return true;
    }
    parts
        .iter()
        .any(|name| SKIP_DIR_NAMES.iter().any(|d| *d == name))
}

fn file_looks_like_log_or_dump(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    name.ends_with(".log")
        || name.ends_with(".dmp")
        || name.ends_with(".tmp")
        || name.ends_with(".mdmp")
        || name.contains("crashdump")
        || name.contains("minidump")
}

fn normalize_title(name: &str) -> String {
    name.replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or(name)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

/// True when this display name or install-folder name is auto-excluded.
pub fn title_is_auto_excluded(name: &str) -> bool {
    let n = normalize_title(name);
    AUTO_EXCLUDE_TITLES.iter().any(|title| n == *title)
}

/// True when any path component is an auto-excluded title.
pub fn path_is_auto_excluded(path: &Path) -> bool {
    let raw = path.to_string_lossy().replace('\\', "/");
    raw.split('/').any(title_is_auto_excluded)
}

/// Display title used when refusing an auto-excluded path.
pub fn auto_excluded_title(path: &Path) -> Option<String> {
    let raw = path.to_string_lossy().replace('\\', "/");
    raw.split('/').find_map(|part| {
        if title_is_auto_excluded(part) {
            Some(part.to_string())
        } else {
            None
        }
    })
}

/// DirectStorage runtime in the tree. Compacting this install can break IO.
pub fn tree_contains_dstorage(root: &Path) -> bool {
    contains_any_file_named(root, DSTORAGE_FILENAMES)
}

fn contains_any_file_named(root: &Path, targets: &[&str]) -> bool {
    walk_all_files(root, 12).into_iter().any(|path| {
        path.file_name()
            .and_then(|n| n.to_str())
            .map(|n| targets.iter().any(|t| n.eq_ignore_ascii_case(t)))
            .unwrap_or(false)
    })
}

/// Walk files without applying the skip-dir prune (used to find DirectStorage DLLs).
fn walk_all_files(root: &Path, max_depth: usize) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    fn rec(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<std::path::PathBuf>) {
        if depth > max_depth {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                rec(&path, depth + 1, max_depth, out);
            } else {
                out.push(path);
            }
        }
    }
    rec(root, 0, max_depth, &mut out);
    out
}

/// Same include set for dry-run/estimate and the real WOF apply pass.
pub const COMPACT_WALK_DEPTH: usize = 24;

/// Files the WOF pass may touch: skip-list directories pruned, skip extensions dropped.
pub fn collect_included_files(root: &Path) -> Vec<std::path::PathBuf> {
    walkdir_limited(root, COMPACT_WALK_DEPTH)
        .into_iter()
        .filter(|path| !should_skip(path))
        .collect()
}

/// Bounded walk so tests and dry-run stay cheap. Production compact also uses this.
pub fn walkdir_limited(root: &Path, max_depth: usize) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    fn rec(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<std::path::PathBuf>) {
        if depth > max_depth {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path_has_skipped_dir(&path) {
                continue;
            }
            if path.is_dir() {
                rec(&path, depth + 1, max_depth, out);
            } else {
                out.push(path);
            }
        }
    }
    rec(root, 0, max_depth, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn skips_listed_video_audio_image_archive_temp_exts() {
        let names = [
            "cut.bik",
            "cut.bk2",
            "cut.bik2",
            "cut.pc_binkvid",
            "clip.mp4",
            "clip.webm",
            "clip.mkv",
            "clip.avi",
            "clip.wmv",
            "clip.flv",
            "clip.mpg",
            "clip.m2v",
            "clip.m4v",
            "clip.vob",
            "clip.usm",
            "clip.ivf",
            "song.mp3",
            "song.ogg",
            "song.wma",
            "song.flac",
            "song.opus",
            "song.m4a",
            "song.aac",
            "song.wem",
            "song.fsb",
            "song.xwma",
            "tex.jpg",
            "tex.jpeg",
            "tex.png",
            "tex.webp",
            "tex.ktx",
            "tex.ktx2",
            "tex.basis",
            "tex.basisu",
            "tex.astc",
            "tex.pvr",
            "tex.crn",
            "tex.tfc",
            "pack.zip",
            "pack.7z",
            "pack.rar",
            "pack.gz",
            "pack.xz",
            "pack.cab",
            "pack.bz2",
            "pack.tgz",
            "pack.lz",
            "pack.txz",
            "pack.dmg",
            "pack.lzx",
            "pack.br",
            "pack.lz4",
            "pack.lzma",
            "pack.zst",
            "pack.zstd",
            "out.log",
            "crash.dmp",
            "scratch.tmp",
        ];
        for name in names {
            assert!(
                should_skip(&PathBuf::from(name)),
                "expected skip for {name}"
            );
            if !matches!(
                name.rsplit_once('.').map(|(_, e)| e),
                Some("log" | "dmp" | "tmp")
            ) {
                assert_eq!(
                    skip_reason(&PathBuf::from(name)),
                    Some(SkipReason::Extension),
                    "{name}"
                );
            }
        }
    }

    #[test]
    fn does_not_hard_skip_wav_dds_bnk() {
        for name in [
            "voice.wav",
            "albedo.dds",
            "events.bnk",
            "SOUND.WAV",
            "A.DDS",
        ] {
            assert!(
                !should_skip(&PathBuf::from(name)),
                "must not hard-skip {name}"
            );
            assert!(!extension_is_skipped(&PathBuf::from(name)), "{name}");
        }
        assert!(!should_skip(&PathBuf::from(
            r"C:\games\Foo\audio\voice.wav"
        )));
        assert!(!should_skip(&PathBuf::from(r"C:\games\Foo\tex\albedo.dds")));
        assert!(!should_skip(&PathBuf::from(
            r"C:\games\Foo\sound\events.bnk"
        )));
    }

    #[test]
    fn skips_shader_cache_and_savegames() {
        assert!(should_skip(&PathBuf::from(
            r"C:\games\Foo\ShaderCache\cache.bin"
        )));
        assert!(should_skip(&PathBuf::from(
            r"C:\games\Foo\shadercache\cache.bin"
        )));
        assert!(should_skip(&PathBuf::from(
            r"C:\games\Foo\SaveGames\slot1.sav"
        )));
        assert!(should_skip(&PathBuf::from(r"D:\Bar\logs\output.log")));
        assert!(should_skip(&PathBuf::from(r"D:\Bar\dumps\crash.dmp")));
    }

    #[test]
    fn skips_pipeline_and_api_cache_folders() {
        for path in [
            r"C:\games\Foo\PipelineCache\pso.bin",
            r"C:\games\Foo\PipelineCaches\pso.bin",
            r"C:\games\Foo\PSOCache\pso.bin",
            r"C:\games\Foo\DXCache\dx.bin",
            r"C:\games\Foo\VkCache\vk.bin",
            r"C:\games\Foo\GLCache\gl.bin",
            r"C:\games\Foo\Saved\PipelineCaches\pso.bin",
        ] {
            assert!(
                should_skip(&PathBuf::from(path)),
                "expected folder skip for {path}"
            );
            assert_eq!(
                skip_reason(&PathBuf::from(path)),
                Some(SkipReason::Directory),
                "{path}"
            );
        }
        assert!(!path_has_skipped_dir(Path::new(
            r"C:\games\Foo\Saved\SaveSlots\slot.sav"
        )));
    }

    #[test]
    fn auto_excludes_guild_wars_2_and_secret_world_legends() {
        assert!(title_is_auto_excluded("Guild Wars 2"));
        assert!(title_is_auto_excluded("guild  wars  2"));
        assert!(title_is_auto_excluded("Secret World Legends"));
        assert!(path_is_auto_excluded(Path::new(
            r"D:\SteamLibrary\steamapps\common\Guild Wars 2"
        )));
        assert!(path_is_auto_excluded(Path::new(
            r"E:\SteamLibrary\steamapps\common\Secret World Legends"
        )));
        assert_eq!(
            auto_excluded_title(Path::new(r"D:\SteamLibrary\steamapps\common\Guild Wars 2"))
                .as_deref(),
            Some("Guild Wars 2")
        );
        assert!(!title_is_auto_excluded("Guild Wars"));
        assert!(!title_is_auto_excluded("Counter-Strike 2"));
        assert!(!path_is_auto_excluded(Path::new(
            r"D:\SteamLibrary\steamapps\common\Counter-Strike Global Offensive"
        )));
    }

    #[test]
    fn detects_dstorage_and_dstoragecore() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "rusticgu-dstorage-{}-{}",
            std::process::id(),
            stamp
        ));
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(root.join("bin").join("game.exe"), b"exe").unwrap();
        assert!(!tree_contains_dstorage(&root));

        std::fs::write(root.join("bin").join("dstoragecore.dll"), b"ds").unwrap();
        assert!(tree_contains_dstorage(&root));

        std::fs::remove_file(root.join("bin").join("dstoragecore.dll")).unwrap();
        std::fs::write(root.join("dstorage.dll"), b"ds").unwrap();
        assert!(tree_contains_dstorage(&root));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn keeps_game_binaries() {
        assert!(!should_skip(&PathBuf::from(r"C:\games\Foo\foo.exe")));
        assert!(!should_skip(&PathBuf::from(r"C:\games\Foo\data\level.dat")));
        assert!(!should_skip(&PathBuf::from(
            r"C:\games\Foo\engine\renderer.dll"
        )));
    }

    #[test]
    fn skips_unity_video_and_music_bundles_keeps_other_bundles() {
        for name in [
            r"C:\games\BloonsTD6\BloonsTD6_Data\StreamingAssets\aa\StandaloneWindows64\Full\videos_assets_all_2ab8.bundle",
            r"C:\games\BloonsTD6\BloonsTD6_Data\StreamingAssets\aa\StandaloneWindows64\Half\music_assets_musictitle_4311.bundle",
            r"D:\SteamLibrary\steamapps\common\Game\Audio_streams.bundle",
            "cutscene_intro.assets",
            "sound_bank.resource",
        ] {
            assert!(
                packed_media_container(Path::new(name)),
                "expected packed-media skip for {name}"
            );
            assert_eq!(
                skip_reason(Path::new(name)),
                Some(SkipReason::PackedMedia),
                "{name}"
            );
        }
        for name in [
            r"C:\games\BloonsTD6\BloonsTD6_Data\StreamingAssets\aa\StandaloneWindows64\Full\asset_references_assets_all_13a2.bundle",
            r"C:\games\BloonsTD6\GameAssembly.dll",
            r"C:\games\BloonsTD6\BloonsTD6_Data\resources.assets",
            r"C:\games\Foo\sharedassets0.assets",
            "sprite_atlases_assets_all_aabb.bundle",
            "AudioMixer.dll",
        ] {
            assert!(
                !packed_media_container(Path::new(name)),
                "must not skip {name}"
            );
            assert!(!should_skip(Path::new(name)), "{name}");
        }
    }

    #[test]
    fn skips_on_disk_files_with_media_magic() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("rusticgu-magic-{}-{}", std::process::id(), stamp));
        std::fs::create_dir_all(&dir).unwrap();
        let mp4 = dir.join("clip.bin");
        let mut bytes = vec![0u8; 16];
        bytes[4..8].copy_from_slice(b"ftyp");
        std::fs::write(&mp4, bytes).unwrap();
        let ogg = dir.join("track.bin");
        std::fs::write(&ogg, b"OggS........").unwrap();
        let plain = dir.join("level.dat");
        std::fs::write(&plain, b"not media!!").unwrap();

        assert_eq!(skip_reason(&mp4), Some(SkipReason::PackedMedia));
        assert_eq!(skip_reason(&ogg), Some(SkipReason::PackedMedia));
        assert!(skip_reason(&plain).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn collect_included_files_matches_skip_list() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root =
            std::env::temp_dir().join(format!("rusticgu-include-{}-{}", std::process::id(), stamp));
        std::fs::create_dir_all(root.join("ShaderCache")).unwrap();
        std::fs::create_dir_all(root.join("data")).unwrap();
        std::fs::write(root.join("data").join("level.dat"), b"d").unwrap();
        std::fs::write(root.join("albedo.dds"), b"dds").unwrap();
        std::fs::write(root.join("clip.mkv"), b"mkv").unwrap();
        std::fs::write(root.join("videos_assets_all_2ab8.bundle"), b"vid").unwrap();
        std::fs::write(root.join("asset_references_assets_all.bundle"), b"ref").unwrap();
        std::fs::write(root.join("ShaderCache").join("pso.bin"), b"p").unwrap();

        let included = collect_included_files(&root);
        let names: Vec<String> = included
            .iter()
            .filter_map(|p| p.file_name()?.to_str().map(|s| s.to_string()))
            .collect();
        assert!(names.contains(&"level.dat".into()));
        assert!(names.contains(&"albedo.dds".into()));
        assert!(names.contains(&"asset_references_assets_all.bundle".into()));
        assert!(!names.iter().any(|n| n == "clip.mkv" || n == "pso.bin"));
        assert!(!names.iter().any(|n| n.starts_with("videos_")));

        let _ = std::fs::remove_dir_all(&root);
    }
}
