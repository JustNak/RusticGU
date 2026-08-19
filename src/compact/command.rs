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

use super::skip::collect_included_files;

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
/// Never adds recursive `/S` — apply uses [`build_apply_invocations`] instead.
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
    let algorithm = algorithm.for_live_library();
    let mut args = Vec::new();
    match op {
        CompactOp::Compress => {
            args.push(OsString::from("/C"));
            args.push(OsString::from(format!("/EXE:{}", algorithm.exe_flag())));
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

/// Apply-set invocations: included files only, never `compact /C /EXE /S <install_root>`.
pub fn build_apply_invocations(
    op: CompactOp,
    root: &Path,
    algorithm: CompactAlgorithm,
) -> Vec<CompactInvocation> {
    let files = collect_included_files(root);
    batch_apply_files(op, &files, algorithm)
}

/// Paths the apply pass will pass to `compact.exe` (skip list already applied).
pub fn apply_target_paths(root: &Path) -> Vec<PathBuf> {
    collect_included_files(root)
}

const APPLY_BATCH_FILES: usize = 48;
const APPLY_BATCH_CHARS: usize = 20_000;

fn batch_apply_files(
    op: CompactOp,
    files: &[PathBuf],
    algorithm: CompactAlgorithm,
) -> Vec<CompactInvocation> {
    let mut out = Vec::new();
    let mut batch: Vec<PathBuf> = Vec::new();
    let mut chars = 0usize;
    for file in files {
        let add = file.as_os_str().len().saturating_add(3);
        if !batch.is_empty()
            && (batch.len() >= APPLY_BATCH_FILES || chars.saturating_add(add) > APPLY_BATCH_CHARS)
        {
            out.push(build_compact_files_command(op, &batch, algorithm));
            batch.clear();
            chars = 0;
        }
        chars = chars.saturating_add(add);
        batch.push(file.clone());
    }
    if !batch.is_empty() {
        out.push(build_compact_files_command(op, &batch, algorithm));
    }
    out
}

/// True when this invocation would recursively WOF the install root (`/S` + root).
pub fn invocation_recurses_install_root(inv: &CompactInvocation, root: &Path) -> bool {
    let args: Vec<String> = inv
        .args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let has_s = args.iter().any(|a| {
        let u = a.to_ascii_uppercase();
        u == "/S" || u.starts_with("/S:")
    });
    if !has_s {
        return false;
    }
    let root_key = normalize_path_key(root);
    args.iter()
        .any(|a| normalize_path_key(Path::new(a)) == root_key)
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
        // `/EXE` must be present so compact.exe does not fall back to LZNT1.
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
}
