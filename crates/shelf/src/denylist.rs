use std::path::Path;

/// A self-rewriting title that must never be compacted (any algorithm).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DenyRule {
    pub reason: String,
    /// Case-insensitive title match (equality, or substring when pattern is long).
    pub names: Vec<String>,
    /// Launcher / store ids (Steam appid, GOG id, folder product codes).
    pub ids: Vec<String>,
    /// Install-folder path components (case-insensitive).
    pub folder_markers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DenyList {
    pub rules: Vec<DenyRule>,
}

impl DenyList {
    pub fn empty() -> Self {
        Self { rules: Vec::new() }
    }

    pub fn extend_with(&mut self, rule: DenyRule) {
        self.rules.push(rule);
    }

    pub fn match_title(
        &self,
        title: &str,
        store_id: Option<&str>,
        launcher_id: Option<&str>,
        install_folder: Option<&Path>,
    ) -> Option<&DenyRule> {
        self.rules.iter().find(|r| {
            r.names.iter().any(|n| names_match(title, n))
                || id_match(store_id, launcher_id, &r.ids)
                || folder_match(install_folder, &r.folder_markers)
        })
    }
}

fn names_match(title: &str, pattern: &str) -> bool {
    let t = title.trim();
    let p = pattern.trim();
    if t.is_empty() || p.is_empty() {
        return false;
    }
    if t.eq_ignore_ascii_case(p) {
        return true;
    }
    // Short tokens (GW2) are equality-only to avoid false positives.
    if p.chars().count() <= 3 {
        return false;
    }
    let tl = t.to_ascii_lowercase();
    let pl = p.to_ascii_lowercase();
    tl.contains(&pl)
}

fn id_match(store_id: Option<&str>, launcher_id: Option<&str>, ids: &[String]) -> bool {
    ids.iter().any(|want| {
        launcher_id.is_some_and(|g| g.eq_ignore_ascii_case(want))
            || store_id.is_some_and(|s| s.eq_ignore_ascii_case(want))
            || match (store_id, launcher_id) {
                (Some(s), Some(g)) => format!("{s}:{g}").eq_ignore_ascii_case(want),
                _ => false,
            }
    })
}

fn folder_match(install: Option<&Path>, markers: &[String]) -> bool {
    let Some(path) = install else {
        return false;
    };
    // Split on both separators so `D:\Games\Guild Wars 2` matches on Linux tests.
    path.to_string_lossy()
        .split(['/', '\\'])
        .filter(|s| !s.is_empty())
        .any(|name| markers.iter().any(|m| name.eq_ignore_ascii_case(m)))
}

/// Confirmed default denylist (researcher-locked).
///
/// * **Guild Wars 2**: ArenaNet self-rewriter (`Gw2-64` / `local.dat`).
/// * **Secret World Legends**: Funcom self-updating client (confirmed).
/// * **The Lord of the Rings Online**: patcher freezes if files stay
///   WOF-compressed; must decompress to patch (must-not-stay-compressed).
///
/// **Not** on the default list: ARK, ESO (unconfirmed). Do not add them here.
pub fn default_denylist() -> DenyList {
    DenyList {
        rules: vec![
            DenyRule {
                reason: "Guild Wars 2 ArenaNet self-rewriter (Gw2-64 / local.dat)".into(),
                names: vec!["Guild Wars 2".into(), "GW2".into()],
                ids: vec!["1284210".into(), "steam:1284210".into()],
                folder_markers: vec!["Guild Wars 2".into(), "Gw2".into(), "GW2".into()],
            },
            DenyRule {
                reason: "Secret World Legends Funcom self-updating client".into(),
                names: vec!["Secret World Legends".into(), "The Secret World".into()],
                ids: vec!["376480".into(), "steam:376480".into()],
                folder_markers: vec!["Secret World Legends".into(), "SecretWorldLegends".into()],
            },
            DenyRule {
                reason: "LOTRO patcher freezes on WOF-compressed files; decompress to patch".into(),
                names: vec![
                    "The Lord of the Rings Online".into(),
                    "Lord of the Rings Online".into(),
                    "LOTRO".into(),
                ],
                ids: vec!["212500".into(), "steam:212500".into()],
                folder_markers: vec![
                    "The Lord of the Rings Online".into(),
                    "Lord of the Rings Online".into(),
                    "LOTRO".into(),
                ],
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn gw2_name_is_case_insensitive() {
        let d = default_denylist();
        assert!(d.match_title("guild wars 2", None, None, None).is_some());
        assert!(d.match_title("GW2", None, None, None).is_some());
        assert!(d
            .match_title("Hades", None, None, Some(Path::new(r"C:\Games\Hades")))
            .is_none());
    }

    #[test]
    fn ark_and_eso_are_not_default_excluded() {
        let d = default_denylist();
        assert!(d
            .match_title("ARK: Survival Evolved", None, None, None)
            .is_none());
        assert!(d.match_title("ARK", None, None, None).is_none());
        assert!(d
            .match_title("The Elder Scrolls Online", None, None, None)
            .is_none());
        assert!(d.match_title("ESO", None, None, None).is_none());
    }
}
