//! `compact.exe` command-line construction.
//!
//! Only WOF `/EXE` compression is allowed. NTFS LZNT1 (`compact` without `/EXE`)
//! must never be generated.

use std::ffi::OsString;
use std::path::Path;

use crate::settings::CompactAlgorithm;

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

/// Build a WOF compact / uncompact command for `root`.
///
/// Always includes `/EXE` (and an algorithm on compress). Never emits LZNT1.
pub fn build_compact_command(
    op: CompactOp,
    root: &Path,
    algorithm: CompactAlgorithm,
) -> CompactInvocation {
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
    args.push(OsString::from("/S"));
    args.push(OsString::from("/I"));
    args.push(OsString::from("/Q"));
    args.push(root.as_os_str().to_os_string());
    CompactInvocation {
        program: OsString::from("compact.exe"),
        args,
    }
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
}
