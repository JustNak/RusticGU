//! `compact.exe` command-line construction.
//!
//! Only WOF `/EXE` compression is allowed. NTFS LZNT1 (`compact` without `/EXE`)
//! must never be generated.
//!
//! Apply never uses recursive `/S` on the install root. The skip-list walker
//! selects included files; each invocation lists those files only.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::settings::CompactAlgorithm;

use super::skip::{collect_included_files, should_skip};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactOp {
    Compress,
    Uncompress,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactInvocation {
    pub program: OsString,
    pub args: Vec<OsString>,
}

impl CompactInvocation {
    pub fn display_cmdline(&self) -> String {
        let mut parts = vec![quote(&self.program)];
        parts.extend(self.args.iter().map(|a| quote(a)));
        parts.join(" ")
    }
}

/// Build a WOF compact / uncompact command for one path (file or explicit target).
///
/// Always includes `/EXE` (and an algorithm on compress). Never emits LZNT1.
/// Never adds recursive `/S`. Apply uses [`build_apply_invocations`] instead.
pub fn build_compact_command(
    op: CompactOp,
    target: &Path,
    algorithm: CompactAlgorithm,
) -> CompactInvocation {
    let path = target.to_path_buf();
    build_compact_files_command(op, std::slice::from_ref(&path), algorithm)
}

/// WOF command for an explicit file list. No `/S`.
pub fn build_compact_files_command(
    op: CompactOp,
    files: &[PathBuf],
    algorithm: CompactAlgorithm,
) -> CompactInvocation {
    build_wof_files_command(op, files, algorithm.for_live_library())
}

/// WOF command using the algorithm as given (Shelf may pass LZX).
pub fn build_wof_files_command(
    op: CompactOp,
    files: &[PathBuf],
    algorithm: CompactAlgorithm,
) -> CompactInvocation {
    build_wof_files_command_with(op, files, algorithm, false)
}

/// WOF command using the algorithm as given, optionally `/F` to rewrite already-compressed files.
///
/// `/F` is for Change-method apply only. Incremental live compact and first compress
/// must keep `force == false` — already-compressed files are skipped by default.
pub fn build_wof_files_command_with(
    op: CompactOp,
    files: &[PathBuf],
    algorithm: CompactAlgorithm,
    force: bool,
) -> CompactInvocation {
    let mut args = Vec::new();
    match op {
        CompactOp::Compress => {
            args.push(OsString::from("/C"));
            args.push(OsString::from(format!("/EXE:{}", algorithm.exe_flag())));
            if force {
                args.push(OsString::from("/F"));
            }
        }
        CompactOp::Uncompress => {
            args.push(OsString::from("/U"));
            args.push(OsString::from("/EXE"));
        }
    }
    args.push(OsString::from("/I"));
    args.push(OsString::from("/Q"));
    for file in files {
        args.push(file.as_os_str().to_os_string());
    }
    CompactInvocation {
        program: OsString::from("compact.exe"),
        args,
    }
}

/// Apply-compress invocations: included files only, never `compact /C /EXE /S <install_root>`.
pub fn build_apply_invocations(
    op: CompactOp,
    root: &Path,
    algorithm: CompactAlgorithm,
) -> Vec<CompactInvocation> {
    build_apply_invocations_with(op, root, algorithm, true)
}

/// Same as [`build_apply_invocations`], optionally keeping Shelf LZX.
pub fn build_apply_invocations_with(
    op: CompactOp,
    root: &Path,
    algorithm: CompactAlgorithm,
    coerce_live: bool,
) -> Vec<CompactInvocation> {
    build_apply_invocations_with_force(op, root, algorithm, coerce_live, false)
}

/// Apply invocations, with `/F` when `force` is set so Change-method can rewrite.
pub fn build_apply_invocations_with_force(
    op: CompactOp,
    root: &Path,
    algorithm: CompactAlgorithm,
    coerce_live: bool,
    force: bool,
) -> Vec<CompactInvocation> {
    let algorithm = if coerce_live {
        algorithm.for_live_library()
    } else {
        algorithm
    };
    match op {
        CompactOp::Compress => {
            let files = collect_included_files(root);
            batch_apply_files(op, &files, algorithm, force)
        }
        CompactOp::Uncompress => vec![build_uncompress_root_command(root)],
    }
}

/// Incremental recompact: named files only. Never `/F`, never `/S` on the install root.
pub fn build_incremental_invocations(
    root: &Path,
    files: &[PathBuf],
    algorithm: CompactAlgorithm,
) -> Vec<CompactInvocation> {
    let algorithm = algorithm.for_live_library();
    let resolved: Vec<PathBuf> = files
        .iter()
        .map(|f| {
            if f.is_absolute() {
                f.clone()
            } else {
                root.join(f)
            }
        })
        .filter(|p| p.is_file() && !should_skip(p))
        .collect();
    batch_apply_files(CompactOp::Compress, &resolved, algorithm, false)
}

/// True when this invocation asks `compact.exe` to force a full rewrite (`/F`).
pub fn invocation_has_force_flag(inv: &CompactInvocation) -> bool {
    inv.args.iter().any(|a| {
        let u = a.to_string_lossy().to_ascii_uppercase();
        u == "/F" || u.starts_with("/F:")
    })
}

/// Undo may recurse the install root. Compress must not.
///
/// `compact /S` with no directory uses the process CWD. Bind the install root
/// as `/S:<dir>` so Decompress cannot walk System32 / the app folder instead.
pub fn build_uncompress_root_command(root: &Path) -> CompactInvocation {
    let mut s_dir = OsString::from("/S:");
    s_dir.push(root.as_os_str());
    CompactInvocation {
        program: OsString::from("compact.exe"),
        args: vec![
            OsString::from("/U"),
            OsString::from("/EXE"),
            s_dir,
            OsString::from("/A"),
            OsString::from("/I"),
            OsString::from("/Q"),
        ],
    }
}

/// Paths the apply pass will pass to `compact.exe` (skip list already applied).
pub fn apply_target_paths(root: &Path) -> Vec<PathBuf> {
    collect_included_files(root)
}

const APPLY_BATCH_FILES: usize = 48;
const LZX_APPLY_BATCH_FILES: usize = 8;
const APPLY_BATCH_CHARS: usize = 20_000;
/// Maximum / LZX: files this large get their own compact.exe so one slow
/// encode cannot stall or fail a 48-file batch (BTD6 Unity bundles are 100MB–1GB).
const LZX_ISOLATE_FILE_BYTES: u64 = 32 * 1024 * 1024;

pub(crate) fn apply_batch_file_limit(algorithm: CompactAlgorithm) -> usize {
    if algorithm == CompactAlgorithm::Lzx {
        LZX_APPLY_BATCH_FILES
    } else {
        APPLY_BATCH_FILES
    }
}

pub(crate) fn isolate_file_in_own_invocation(algorithm: CompactAlgorithm, file_len: u64) -> bool {
    algorithm == CompactAlgorithm::Lzx && file_len >= LZX_ISOLATE_FILE_BYTES
}

/// Paths this invocation will pass to compact.exe (flags skipped).
pub fn invocation_target_files(inv: &CompactInvocation) -> Vec<PathBuf> {
    inv.args
        .iter()
        .filter(|a| !a.to_string_lossy().starts_with('/'))
        .map(PathBuf::from)
        .collect()
}

fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn batch_apply_files(
    op: CompactOp,
    files: &[PathBuf],
    algorithm: CompactAlgorithm,
    force: bool,
) -> Vec<CompactInvocation> {
    let max_files = apply_batch_file_limit(algorithm);
    let mut out = Vec::new();
    let mut batch: Vec<PathBuf> = Vec::new();
    let mut chars = 0usize;
    let flush = |batch: &mut Vec<PathBuf>, out: &mut Vec<CompactInvocation>| {
        if !batch.is_empty() {
            out.push(build_wof_files_command_with(op, batch, algorithm, force));
            batch.clear();
        }
    };
    for file in files {
        let add = file.as_os_str().len().saturating_add(3);
        let isolate = isolate_file_in_own_invocation(algorithm, file_len(file));
        if !batch.is_empty()
            && (isolate
                || batch.len() >= max_files
                || chars.saturating_add(add) > APPLY_BATCH_CHARS)
        {
            flush(&mut batch, &mut out);
            chars = 0;
        }
        chars = chars.saturating_add(add);
        batch.push(file.clone());
        if isolate {
            flush(&mut batch, &mut out);
            chars = 0;
        }
    }
    flush(&mut batch, &mut out);
    out
}

/// True when this invocation recursively WOF-operates the install root (`/S:<root>`).
///
/// A bare `/S` is CWD recursion and does **not** count — that is the Decompress bug.
pub fn invocation_recurses_install_root(inv: &CompactInvocation, root: &Path) -> bool {
    let root_key = normalize_path_key(root);
    inv.args.iter().any(|a| {
        let s = a.to_string_lossy();
        s_flag_directory(&s).is_some_and(|dir| normalize_path_key(Path::new(dir)) == root_key)
    })
}

/// Directory bound by `/S:dir`. `None` for a bare `/S` (CWD).
fn s_flag_directory(arg: &str) -> Option<&str> {
    let bytes = arg.as_bytes();
    if bytes.len() > 3
        && bytes[0] == b'/'
        && (bytes[1] == b'S' || bytes[1] == b's')
        && bytes[2] == b':'
    {
        Some(&arg[3..])
    } else {
        None
    }
}

fn normalize_path_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

fn quote(value: &OsString) -> String {
    let s = value.to_string_lossy();
    if s.chars().any(|c| c.is_whitespace()) {
        format!("\"{s}\"")
    } else {
        s.into_owned()
    }
}

/// True when the constructed command is a WOF `/EXE` invocation.
pub fn is_wof_exe_command(inv: &CompactInvocation) -> bool {
    let args: Vec<String> = inv
        .args
        .iter()
        .map(|a| a.to_string_lossy().to_ascii_uppercase())
        .collect();
    args.iter().any(|a| a == "/EXE" || a.starts_with("/EXE:"))
}

/// True if the command would use legacy NTFS LZNT1 (forbidden).
pub fn is_lznt1_command(inv: &CompactInvocation) -> bool {
    !is_wof_exe_command(inv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn compress_uses_exe_xpress8k_never_lznt1() {
        let inv = build_compact_command(
            CompactOp::Compress,
            &PathBuf::from(r"C:\games\Foo"),
            CompactAlgorithm::Xpress8k,
        );
        let line = inv.display_cmdline().to_ascii_uppercase();
        assert!(line.contains("COMPACT"));
        assert!(line.contains("/C"));
        assert!(line.contains("/EXE:XPRESS8K"));
        assert!(is_wof_exe_command(&inv));
        assert!(!is_lznt1_command(&inv));
        assert!(!line.contains("LZNT1"));
        assert!(inv
            .args
            .iter()
            .any(|a| a.to_string_lossy().to_ascii_uppercase().starts_with("/EXE")));
    }

    #[test]
    fn uncompress_uses_u_and_exe() {
        let inv = build_compact_command(
            CompactOp::Uncompress,
            &PathBuf::from(r"D:\SteamLibrary\steamapps\common\Bar"),
            CompactAlgorithm::Xpress8k,
        );
        let args: Vec<String> = inv
            .args
            .iter()
            .map(|a| a.to_string_lossy().to_ascii_uppercase())
            .collect();
        assert!(args.iter().any(|a| a == "/U"));
        assert!(args.iter().any(|a| a == "/EXE" || a.starts_with("/EXE:")));
        assert!(is_wof_exe_command(&inv));
        assert!(!is_lznt1_command(&inv));
    }

    #[test]
    fn other_algorithms_still_use_exe() {
        for algo in CompactAlgorithm::ALL {
            let inv = build_compact_command(CompactOp::Compress, Path::new("."), algo);
            assert!(is_wof_exe_command(&inv), "{algo:?}");
            assert!(!is_lznt1_command(&inv), "{algo:?}");
        }
    }

    #[test]
    fn live_library_command_never_selects_lzx() {
        for algo in CompactAlgorithm::LIVE {
            let inv = build_compact_command(CompactOp::Compress, Path::new("."), algo);
            let line = inv.display_cmdline().to_ascii_uppercase();
            assert!(!line.contains("LZX"), "{line}");
            assert!(is_wof_exe_command(&inv));
        }
        let coerced = CompactAlgorithm::Lzx.for_live_library();
        let inv = build_compact_command(CompactOp::Compress, Path::new("."), coerced);
        assert!(inv
            .display_cmdline()
            .to_ascii_uppercase()
            .contains("/EXE:XPRESS8K"));
        assert!(!inv.display_cmdline().to_ascii_uppercase().contains("LZX"));
    }

    #[test]
    fn apply_compress_command_is_not_recursive_on_install_root() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root =
            std::env::temp_dir().join(format!("rusticgu-qa-nors-{}-{}", std::process::id(), stamp));
        std::fs::create_dir_all(root.join("SaveGames")).unwrap();
        std::fs::create_dir_all(root.join("ShaderCache")).unwrap();
        std::fs::write(root.join("game.exe"), b"exe").unwrap();
        std::fs::write(root.join("movie.mp4"), b"vid").unwrap();
        std::fs::write(root.join("archive.zip"), b"zip").unwrap();
        std::fs::write(root.join("SaveGames").join("slot.sav"), b"save").unwrap();
        std::fs::write(root.join("ShaderCache").join("x.bin"), b"sh").unwrap();

        let invs = build_apply_invocations(CompactOp::Compress, &root, CompactAlgorithm::Xpress8k);
        assert!(!invs.is_empty());
        let mut joined = String::new();
        for inv in &invs {
            let line = inv.display_cmdline();
            joined.push_str(&line);
            joined.push('\n');
            assert!(
                !invocation_recurses_install_root(inv, &root),
                "compress must not be /S on install root: {line}"
            );
            let upper = line.to_ascii_uppercase();
            assert!(
                !upper.contains("/S"),
                "compress apply must not emit /S: {line}"
            );
            assert!(is_wof_exe_command(inv));
        }
        let lower = joined.replace('\\', "/").to_ascii_lowercase();
        for forbidden in [
            "movie.mp4",
            "slot.sav",
            "x.bin",
            "archive.zip",
            "savegames",
            "shadercache",
        ] {
            assert!(
                !lower.contains(forbidden),
                "skipped path {forbidden} must not be a compact target: {joined}"
            );
        }
        assert!(lower.contains("game.exe"), "{joined}");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_include_set_excludes_should_skip() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "rusticgu-qa-include-{}-{}",
            std::process::id(),
            stamp
        ));
        std::fs::create_dir_all(root.join("SaveGames")).unwrap();
        std::fs::create_dir_all(root.join("ShaderCache")).unwrap();
        std::fs::write(root.join("game.exe"), b"exe").unwrap();
        std::fs::write(root.join("video.mp4"), b"vid").unwrap();
        std::fs::write(root.join("SaveGames").join("a.sav"), b"save").unwrap();
        std::fs::write(root.join("ShaderCache").join("c.bin"), b"sh").unwrap();
        std::fs::write(root.join("foo.log"), b"log").unwrap();

        let included = apply_target_paths(&root);
        let names: Vec<String> = included
            .iter()
            .filter_map(|p| p.file_name()?.to_str().map(|s| s.to_ascii_lowercase()))
            .collect();
        assert_eq!(names, vec!["game.exe".to_string()]);
        for skipped in ["video.mp4", "a.sav", "c.bin", "foo.log"] {
            assert!(
                !names.iter().any(|n| n == skipped),
                "include set must not contain {skipped}: {names:?}"
            );
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_builder_does_not_s_the_install_root_and_omits_skipped_paths() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "rusticgu-apply-set-{}-{}",
            std::process::id(),
            stamp
        ));
        std::fs::create_dir_all(root.join("SaveGames")).unwrap();
        std::fs::create_dir_all(root.join("ShaderCache")).unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(root.join("bin").join("game.exe"), b"exe").unwrap();
        std::fs::write(root.join("movie.mp4"), b"vid").unwrap();
        std::fs::write(root.join("track.mp3"), b"aud").unwrap();
        std::fs::write(root.join("tex.png"), b"png").unwrap();
        std::fs::write(root.join("pack.zip"), b"zip").unwrap();
        std::fs::write(root.join("out.log"), b"log").unwrap();
        std::fs::write(root.join("voice.wav"), b"wav").unwrap();
        std::fs::write(root.join("SaveGames").join("slot.sav"), b"save").unwrap();
        std::fs::write(root.join("ShaderCache").join("cache.bin"), b"sh").unwrap();

        let invs = build_apply_invocations(CompactOp::Compress, &root, CompactAlgorithm::Xpress8k);
        assert!(
            !invs.is_empty(),
            "included files should produce at least one invocation"
        );

        let mut apply_args = Vec::new();
        for inv in &invs {
            let line = inv.display_cmdline().to_ascii_uppercase();
            assert!(!line.contains("/S"), "apply must not use /S: {line}");
            assert!(
                !invocation_recurses_install_root(inv, &root),
                "must not emit compact /C /EXE /S <install_root>: {line}"
            );
            assert!(is_wof_exe_command(inv));
            assert!(!is_lznt1_command(inv));
            for arg in &inv.args {
                apply_args.push(
                    arg.to_string_lossy()
                        .replace('\\', "/")
                        .to_ascii_lowercase(),
                );
            }
        }

        let joined = apply_args.join("\n");
        assert!(
            apply_args.iter().any(|a| a.ends_with("/game.exe")),
            "game.exe must be in the apply set: {joined}"
        );
        assert!(
            apply_args.iter().any(|a| a.ends_with("/voice.wav")),
            "wav must not be hard-skipped: {joined}"
        );
        for forbidden in [
            "movie.mp4",
            "track.mp3",
            "tex.png",
            "pack.zip",
            "out.log",
            "slot.sav",
            "cache.bin",
            "savegames",
            "shadercache",
        ] {
            assert!(
                !joined.contains(forbidden),
                "skipped path {forbidden} must not be in the apply set: {joined}"
            );
        }

        let targets = apply_target_paths(&root);
        assert!(targets
            .iter()
            .any(|p| p.file_name().and_then(|n| n.to_str()) == Some("game.exe")));
        assert!(targets
            .iter()
            .any(|p| p.file_name().and_then(|n| n.to_str()) == Some("voice.wav")));
        assert!(!targets.iter().any(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("mp4"))
        }));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn incremental_recompact_has_no_force_or_root_s() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "rusticgu-incr-cmd-{}-{}",
            std::process::id(),
            stamp
        ));
        std::fs::create_dir_all(root.join("SaveGames")).unwrap();
        std::fs::write(root.join("new_patch.vpk"), b"vpk").unwrap();
        std::fs::write(root.join("movie.mp4"), b"vid").unwrap();
        std::fs::write(root.join("SaveGames").join("slot.sav"), b"save").unwrap();

        let invs = build_incremental_invocations(
            &root,
            &[
                PathBuf::from("new_patch.vpk"),
                PathBuf::from("movie.mp4"),
                PathBuf::from("SaveGames").join("slot.sav"),
            ],
            CompactAlgorithm::Xpress8k,
        );
        assert!(!invs.is_empty());
        for inv in &invs {
            let line = inv.display_cmdline().to_ascii_uppercase();
            assert!(!invocation_has_force_flag(inv), "{line}");
            assert!(!line.contains("/F"), "{line}");
            assert!(!line.contains("/S"), "{line}");
            assert!(!invocation_recurses_install_root(inv, &root), "{line}");
            assert!(is_wof_exe_command(inv));
            assert!(!is_lznt1_command(inv));
            assert!(line.contains("/EXE:XPRESS8K"), "{line}");
        }
        let joined = invs
            .iter()
            .map(|i| i.display_cmdline().replace('\\', "/").to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("new_patch.vpk"), "{joined}");
        assert!(!joined.contains("movie.mp4"), "{joined}");
        assert!(!joined.contains("slot.sav"), "{joined}");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn uncompress_root_binds_s_to_install_dir() {
        let root = PathBuf::from(r"D:\SteamLibrary\steamapps\common\Bar Game");
        let inv = build_uncompress_root_command(&root);
        let args: Vec<String> = inv
            .args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(
            !args.iter().any(|a| a.eq_ignore_ascii_case("/S")),
            "bare /S walks CWD: {args:?}"
        );
        assert!(
            args.iter().any(|a| s_flag_directory(a).is_some_and(|d| {
                normalize_path_key(Path::new(d)) == normalize_path_key(&root)
            })),
            "expected /S:<install root>, got {args:?}"
        );
        assert!(args.iter().any(|a| a.eq_ignore_ascii_case("/U")));
        assert!(args.iter().any(|a| a.eq_ignore_ascii_case("/EXE")));
        assert!(args.iter().any(|a| a.eq_ignore_ascii_case("/A")));
        assert!(is_wof_exe_command(&inv));
        assert!(!is_lznt1_command(&inv));
        assert!(invocation_recurses_install_root(&inv, &root));
        assert!(!args
            .iter()
            .any(|a| normalize_path_key(Path::new(a)) == normalize_path_key(&root)));
    }

    #[test]
    fn bare_s_plus_root_filename_does_not_count_as_bound() {
        let root = PathBuf::from(r"C:\games\Foo");
        let inv = CompactInvocation {
            program: OsString::from("compact.exe"),
            args: vec![
                OsString::from("/U"),
                OsString::from("/EXE"),
                OsString::from("/S"),
                OsString::from("/I"),
                OsString::from("/Q"),
                root.as_os_str().to_os_string(),
            ],
        };
        assert!(!invocation_recurses_install_root(&inv, &root));
    }

    #[test]
    fn change_method_apply_uses_force_and_keeps_algorithm() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "rusticgu-change-cmd-{}-{}",
            std::process::id(),
            stamp
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("play.exe"), vec![0u8; 64]).unwrap();

        let invs = build_apply_invocations_with_force(
            CompactOp::Compress,
            &root,
            CompactAlgorithm::Xpress16k,
            false,
            true,
        );
        assert!(!invs.is_empty());
        for inv in &invs {
            let line = inv.display_cmdline().to_ascii_uppercase();
            assert!(invocation_has_force_flag(inv), "{line}");
            assert!(line.contains("/F"), "{line}");
            assert!(line.contains("/EXE:XPRESS16K"), "{line}");
            assert!(!line.contains("/S"), "{line}");
            assert!(is_wof_exe_command(inv));
            assert!(!is_lznt1_command(inv));
        }

        let first = build_apply_invocations_with(
            CompactOp::Compress,
            &root,
            CompactAlgorithm::Xpress16k,
            false,
        );
        for inv in &first {
            assert!(!invocation_has_force_flag(inv), "{}", inv.display_cmdline());
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lzx_uses_smaller_batches_and_isolates_large_files() {
        assert_eq!(apply_batch_file_limit(CompactAlgorithm::Xpress8k), 48);
        assert_eq!(apply_batch_file_limit(CompactAlgorithm::Lzx), 8);
        assert!(!isolate_file_in_own_invocation(
            CompactAlgorithm::Xpress8k,
            32 * 1024 * 1024
        ));
        assert!(!isolate_file_in_own_invocation(
            CompactAlgorithm::Lzx,
            32 * 1024 * 1024 - 1
        ));
        assert!(isolate_file_in_own_invocation(
            CompactAlgorithm::Lzx,
            32 * 1024 * 1024
        ));

        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "rusticgu-lzx-batch-{}-{}",
            std::process::id(),
            stamp
        ));
        std::fs::create_dir_all(&root).unwrap();
        for i in 0..12 {
            std::fs::write(root.join(format!("f{i}.dat")), vec![0u8; 32]).unwrap();
        }

        let xpress = build_apply_invocations_with(
            CompactOp::Compress,
            &root,
            CompactAlgorithm::Xpress8k,
            false,
        );
        let lzx =
            build_apply_invocations_with(CompactOp::Compress, &root, CompactAlgorithm::Lzx, false);
        assert_eq!(xpress.len(), 1, "12 tiny files fit in one XPRESS batch");
        assert!(
            lzx.len() >= 2,
            "LZX batches 8 files so 12 files need two invocations, got {}",
            lzx.len()
        );
        for inv in &lzx {
            let n = invocation_target_files(inv).len();
            assert!(n <= 8, "{n}");
            assert!(inv
                .display_cmdline()
                .to_ascii_uppercase()
                .contains("/EXE:LZX"));
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_skips_packed_video_music_bundles() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "rusticgu-bundle-skip-{}-{}",
            std::process::id(),
            stamp
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("GameAssembly.dll"), b"dll").unwrap();
        std::fs::write(root.join("videos_assets_all_2ab8.bundle"), b"vid").unwrap();
        std::fs::write(root.join("music_assets_musictitle_4311.bundle"), b"mus").unwrap();
        std::fs::write(root.join("asset_references_assets_all_13a2.bundle"), b"ref").unwrap();

        let names: Vec<String> = apply_target_paths(&root)
            .iter()
            .filter_map(|p| p.file_name()?.to_str().map(|s| s.to_string()))
            .collect();
        assert!(names.iter().any(|n| n == "GameAssembly.dll"));
        assert!(names
            .iter()
            .any(|n| n == "asset_references_assets_all_13a2.bundle"));
        assert!(!names.iter().any(|n| n.starts_with("videos_")));
        assert!(!names.iter().any(|n| n.starts_with("music_")));

        let _ = std::fs::remove_dir_all(&root);
    }
}
