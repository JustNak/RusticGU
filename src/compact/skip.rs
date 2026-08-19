//! Files and folders the compact engine must leave alone.

use std::path::Path;

/// Video / audio / archive extensions that should not be WOF-compressed.
const SKIP_EXTENSIONS: &[&str] = &[
    // video
    "mp4", "mkv", "webm", "avi", "mov", "wmv", "m4v", "mpg", "mpeg", "m2v", "bik", "bk2", "usm",
    "ogv", "flv", // audio
    "mp3", "wav", "flac", "ogg", "wma", "aac", "m4a", "opus", "aiff", "aif",
    // archives / already-compressed packages
    "zip", "7z", "rar", "tar", "gz", "tgz", "bz2", "xz", "cab", "iso", "vpk", "pak", "wad", "arc",
    "lz4", "zst", "zstd", // logs / dumps
    "log", "dmp", "mdmp", "hdmp", "wer",
];

/// Directory names (case-insensitive) to skip entirely.
const SKIP_DIR_NAMES: &[&str] = &[
    "savegames",
    "saved games",
    "saves",
    "shadercache",
    "shader cache",
    "nv_cache",
    "dxcache",
    "gpucache",
    "logs",
    "log",
    "dumps",
    "crashdumps",
    "crash_dumps",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    Extension,
    Directory,
    LogDumpName,
}

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
    None
}

pub fn extension_is_skipped(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.trim_start_matches('.').to_ascii_lowercase());
    match ext {
        Some(ext) => SKIP_EXTENSIONS.iter().any(|s| *s == ext),
        None => false,
    }
}

pub fn path_has_skipped_dir(path: &Path) -> bool {
    // Split on both separators so Windows-style fixtures still match on Linux CI hosts.
    let raw = path.to_string_lossy().replace('\\', "/");
    raw.split('/').filter(|part| !part.is_empty()).any(|name| {
        SKIP_DIR_NAMES
            .iter()
            .any(|d| *d == name.to_ascii_lowercase())
    })
}

fn file_looks_like_log_or_dump(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    name.ends_with(".log")
        || name.ends_with(".dmp")
        || name.ends_with(".mdmp")
        || name.contains("crashdump")
        || name.contains("minidump")
}

/// DirectStorage runtime in the tree — compacting this install can break IO.
pub fn tree_contains_dstorage(root: &Path) -> bool {
    contains_file_named(root, "dstorage.dll")
}

fn contains_file_named(root: &Path, target: &str) -> bool {
    let walker = walkdir_limited(root, 8);
    walker.into_iter().any(|path| {
        path.file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.eq_ignore_ascii_case(target))
            .unwrap_or(false)
    })
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
    use std::path::PathBuf;

    #[test]
    fn skips_video_audio_archives() {
        for name in [
            "clip.mp4",
            "song.flac",
            "pack.zip",
            "chunk.vpk",
            "movie.bik",
        ] {
            assert!(
                should_skip(&PathBuf::from(name)),
                "expected skip for {name}"
            );
        }
    }

    #[test]
    fn skips_shader_cache_and_savegames() {
        assert!(should_skip(&PathBuf::from(
            r"C:\games\Foo\ShaderCache\cache.bin"
        )));
        assert!(should_skip(&PathBuf::from(
            r"C:\games\Foo\SaveGames\slot1.sav"
        )));
        assert!(should_skip(&PathBuf::from(r"D:\Bar\logs\output.log")));
    }

    #[test]
    fn keeps_game_binaries() {
        assert!(!should_skip(&PathBuf::from(r"C:\games\Foo\foo.exe")));
        assert!(!should_skip(&PathBuf::from(r"C:\games\Foo\data\level.dat")));
        assert!(!should_skip(&PathBuf::from(
            r"C:\games\Foo\engine\renderer.dll"
        )));
    }
}
