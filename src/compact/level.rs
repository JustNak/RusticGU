//! User-facing compact strength for the library compress flow.
//!
//! Maximum = LZX is a **per-action** override. The live Settings picker stays
//! XPRESS-only; Shelf policy still owns automatic LZX.

use std::path::Path;

use crate::settings::CompactAlgorithm;

use super::engine::{preflight, CompactRefuse};

/// Four-choice compact strength shown in the library compress flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompactLevel {
    /// Fastest, least space back.
    Low,
    /// Default balance.
    #[default]
    Medium,
    /// Stronger XPRESS. Still live-safe (no LZX).
    High,
    /// Smallest on disk. Explicit LZX for this action only.
    Maximum,
}

impl CompactLevel {
    pub const ALL: [CompactLevel; 4] = [
        CompactLevel::Low,
        CompactLevel::Medium,
        CompactLevel::High,
        CompactLevel::Maximum,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Maximum => "Maximum",
        }
    }

    pub fn tradeoff(self) -> &'static str {
        match self {
            Self::Low => "Faster. Uses more space.",
            Self::Medium => "Balanced. Recommended.",
            Self::High => "Smaller. Slower.",
            Self::Maximum => "Smallest. Slowest.",
        }
    }

    pub fn recommended(self) -> bool {
        matches!(self, Self::Medium)
    }

    pub fn icon_path(self) -> &'static str {
        match self {
            Self::Low => "icons/arrow-down.svg",
            Self::Medium => "icons/minus.svg",
            Self::High => "icons/arrow-up.svg",
            Self::Maximum => "icons/file-archive.svg",
        }
    }

    /// WOF algorithm for this choice. Maximum is LZX and must not be coerced.
    pub fn algorithm(self) -> CompactAlgorithm {
        match self {
            Self::Low => CompactAlgorithm::Xpress4k,
            Self::Medium => CompactAlgorithm::Xpress8k,
            Self::High => CompactAlgorithm::Xpress16k,
            Self::Maximum => CompactAlgorithm::Lzx,
        }
    }

    /// Typical on-disk / logical ratio used for the choose-step hint.
    pub fn estimate_ratio(self) -> f64 {
        super::engine::estimate_ratio(self.algorithm())
    }

    /// True when apply must use [`super::apply_compact_allowing_lzx`].
    pub fn allows_lzx(self) -> bool {
        matches!(self, Self::Maximum)
    }
}

/// Per-title outcome before `compact.exe` is spawned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactApplyDecision {
    Apply,
    /// DirectStorage present and override is off: warn and skip this title.
    SkipDirectStorage,
    Refuse(String),
}

/// Classify a title's install root. Skip-list files are still honored on apply.
pub fn decide_compact_apply(root: &Path, allow_dstorage: bool) -> CompactApplyDecision {
    match preflight(root, allow_dstorage) {
        Ok(()) => CompactApplyDecision::Apply,
        Err(CompactRefuse::DirectStorage { .. }) => CompactApplyDecision::SkipDirectStorage,
        Err(err) => CompactApplyDecision::Refuse(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compact::command::{
        apply_target_paths, build_apply_invocations_with, invocation_recurses_install_root,
        is_lznt1_command, is_wof_exe_command, CompactOp,
    };
    use crate::compact::engine::apply_compact_allowing_lzx;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn stamp() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    }

    #[test]
    fn levels_map_to_xpress4k_8k_16k_lzx() {
        assert_eq!(CompactLevel::Low.algorithm(), CompactAlgorithm::Xpress4k);
        assert_eq!(CompactLevel::Low.algorithm().exe_flag(), "XPRESS4K");
        assert_eq!(CompactLevel::Medium.algorithm(), CompactAlgorithm::Xpress8k);
        assert_eq!(CompactLevel::Medium.algorithm().exe_flag(), "XPRESS8K");
        assert_eq!(CompactLevel::High.algorithm(), CompactAlgorithm::Xpress16k);
        assert_eq!(CompactLevel::High.algorithm().exe_flag(), "XPRESS16K");
        assert_eq!(CompactLevel::Maximum.algorithm(), CompactAlgorithm::Lzx);
        assert_eq!(CompactLevel::Maximum.algorithm().exe_flag(), "LZX");
        assert!(!CompactLevel::Low.allows_lzx());
        assert!(!CompactLevel::Medium.allows_lzx());
        assert!(!CompactLevel::High.allows_lzx());
        assert!(CompactLevel::Maximum.allows_lzx());
        assert_eq!(CompactLevel::Low.tradeoff().split('.').count(), 3);
        assert!(CompactLevel::ALL.iter().all(|l| l.tradeoff().len() < 40));
        assert_eq!(CompactLevel::ALL.len(), 4);
        assert_eq!(CompactLevel::default(), CompactLevel::Medium);
        assert!(CompactLevel::Medium.recommended());
        assert!(!CompactLevel::High.recommended());
        assert!(!CompactLevel::Maximum.recommended());
        assert!(CompactLevel::Low.estimate_ratio() > CompactLevel::Medium.estimate_ratio());
        assert!(CompactLevel::Medium.estimate_ratio() > CompactLevel::High.estimate_ratio());
        assert!(CompactLevel::High.estimate_ratio() > CompactLevel::Maximum.estimate_ratio());
        for level in CompactLevel::ALL {
            let blob = format!("{} {}", level.label(), level.tradeoff()).to_ascii_uppercase();
            assert!(!blob.contains("XPRESS"), "{blob}");
            assert!(!blob.contains("LZX"), "{blob}");
        }
        assert_eq!(
            CompactLevel::Maximum.algorithm().for_live_library(),
            CompactAlgorithm::Xpress8k
        );
        assert_eq!(
            CompactLevel::High.algorithm().for_live_library(),
            CompactAlgorithm::Xpress16k
        );
    }

    #[test]
    fn high_lzx_apply_uses_include_set_not_root_s() {
        let root = std::env::temp_dir().join(format!(
            "rusticgu-level-lzx-{}-{}",
            std::process::id(),
            stamp()
        ));
        std::fs::create_dir_all(root.join("SaveGames")).unwrap();
        std::fs::write(root.join("play.exe"), vec![0u8; 64]).unwrap();
        std::fs::write(root.join("cut.mp4"), vec![0u8; 64]).unwrap();
        std::fs::write(root.join("SaveGames").join("x.sav"), b"s").unwrap();

        let algo = CompactLevel::Maximum.algorithm();
        let invs = build_apply_invocations_with(CompactOp::Compress, &root, algo, false);
        assert!(!invs.is_empty());
        for inv in &invs {
            let line = inv.display_cmdline().to_ascii_uppercase();
            assert!(!invocation_recurses_install_root(inv, &root), "{line}");
            assert!(!line.contains("/S"), "{line}");
            assert!(line.contains("/EXE:LZX"), "{line}");
            assert!(is_wof_exe_command(inv));
            assert!(!is_lznt1_command(inv));
            assert!(!line.contains("CUT.MP4"), "{line}");
            assert!(!line.contains("SAVEGAMES"), "{line}");
        }
        let targets = apply_target_paths(&root);
        assert_eq!(targets.len(), 1);
        assert_eq!(
            targets[0].file_name().and_then(|n| n.to_str()),
            Some("play.exe")
        );

        let result =
            apply_compact_allowing_lzx(CompactOp::Compress, &root, algo, false, |_| {}).unwrap();
        assert!(result.ok);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn mid_update_refuse_still_fires_for_dialog_apply() {
        let library = std::env::temp_dir().join(format!(
            "rusticgu-level-upd-{}-{}",
            std::process::id(),
            stamp()
        ));
        let install = library.join("steamapps").join("common").join("BarGame");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join("game.exe"), b"exe").unwrap();
        std::fs::write(
            library.join("steamapps").join("appmanifest_99.acf"),
            r#"
"AppState"
{
	"appid"		"99"
	"name"		"Bar Game"
	"StateFlags"		"4"
	"installdir"		"BarGame"
}
"#,
        )
        .unwrap();
        assert_eq!(
            decide_compact_apply(&install, false),
            CompactApplyDecision::Apply
        );

        std::fs::create_dir_all(library.join("steamapps").join("downloading").join("99")).unwrap();
        match decide_compact_apply(&install, false) {
            CompactApplyDecision::Refuse(msg) => {
                assert!(msg.to_ascii_lowercase().contains("updating"), "{msg}");
            }
            other => panic!("expected refuse, got {other:?}"),
        }

        let apply_err = apply_compact_allowing_lzx(
            CompactOp::Compress,
            &install,
            CompactLevel::Maximum.algorithm(),
            false,
            |_| {},
        )
        .unwrap_err();
        assert!(apply_err.contains("updating"), "{apply_err}");

        let _ = std::fs::remove_dir_all(&library);
    }

    #[test]
    fn dstorage_warn_and_skip_still_fires() {
        let root = std::env::temp_dir().join(format!(
            "rusticgu-level-ds-{}-{}",
            std::process::id(),
            stamp()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("game.exe"), b"exe").unwrap();
        std::fs::write(root.join("dstorage.dll"), b"ds").unwrap();
        std::fs::write(root.join("dstoragecore.dll"), b"ds").unwrap();

        assert_eq!(
            decide_compact_apply(&root, false),
            CompactApplyDecision::SkipDirectStorage
        );
        let err = apply_compact_allowing_lzx(
            CompactOp::Compress,
            &root,
            CompactLevel::Medium.algorithm(),
            false,
            |_| {},
        )
        .unwrap_err();
        assert!(
            err.to_ascii_lowercase().contains("dstorage"),
            "apply must still see dstorage: {err}"
        );

        // Override still allowed: decision becomes Apply (may fail later on non-NTFS CI).
        match decide_compact_apply(&root, true) {
            CompactApplyDecision::Apply | CompactApplyDecision::Refuse(_) => {}
            CompactApplyDecision::SkipDirectStorage => {
                panic!("override must not skip as DirectStorage")
            }
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn live_picker_still_excludes_lzx() {
        let labels: Vec<&'static str> = CompactAlgorithm::LIVE.iter().map(|a| a.label()).collect();
        assert!(!labels.contains(&"LZX"));
        assert_eq!(CompactLevel::Maximum.algorithm().label(), "LZX");
        assert_eq!(CompactLevel::High.algorithm().label(), "XPRESS16K");
        let _ = PathBuf::new();
    }
}
