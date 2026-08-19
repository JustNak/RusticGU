use std::path::Path;

use serde::Deserialize;

use crate::error::{StoreError, StoreWarning};
use crate::fs::IndexFs;
use crate::model::{DiscoveredTitle, StoreId};

/// Agent / Battle.net app — not games. Skip `agent` and `bna` only
/// (plus the `battle.net` alias of `bna`).
const NOT_GAMES: &[&str] = &["agent", "bna", "battle.net"];

/// Battle.net Agent `product_installs[]`:
/// `uid`, `product_code`, `settings.install_path`,
/// `cached_product_state…installed/playable`.
///
/// JSON indexes only (fixture-friendly). Skip `agent` and `bna`.
pub fn discover(fs: &impl IndexFs, agent_dir: &Path) -> (Vec<DiscoveredTitle>, Vec<StoreWarning>) {
    let mut titles = Vec::new();
    let mut warnings = Vec::new();
    if !fs.exists(agent_dir) {
        return (titles, warnings);
    }
    let mut files = Vec::new();
    if fs.is_dir(agent_dir) {
        match fs.list_dir(agent_dir) {
            Ok(entries) => {
                for e in entries {
                    if e.is_dir {
                        continue;
                    }
                    let n = e.name.to_ascii_lowercase();
                    if n.ends_with(".json") {
                        files.push(e.path);
                    }
                }
            }
            Err(err) => {
                warnings.push(StoreWarning::new(StoreId::Battlenet, err.to_string()));
                return (titles, warnings);
            }
        }
    } else {
        files.push(agent_dir.to_path_buf());
    }

    let mut seen = std::collections::BTreeSet::new();
    for path in files {
        match read_product_json(fs, &path) {
            Ok(found) => {
                for t in found {
                    let key = t
                        .launcher_id
                        .clone()
                        .unwrap_or_else(|| t.install_path.display().to_string());
                    if seen.insert(key) {
                        titles.push(t);
                    }
                }
            }
            Err(err) if err.is_not_found() => {}
            Err(err) => warnings.push(StoreWarning::new(StoreId::Battlenet, err.to_string())),
        }
    }
    (titles, warnings)
}

#[derive(Debug, Deserialize)]
struct ProductFile {
    #[serde(default)]
    product_installs: Vec<ProductInstall>,
}

#[derive(Debug, Deserialize, Default)]
struct ProductInstall {
    #[serde(default)]
    uid: Option<String>,
    #[serde(default, alias = "productCode")]
    product_code: Option<String>,
    #[serde(default)]
    settings: Option<ProductSettings>,
    #[serde(default, alias = "cachedProductState")]
    cached_product_state: Option<CachedProductState>,
}

#[derive(Debug, Deserialize, Default)]
struct ProductSettings {
    #[serde(default, alias = "installPath")]
    install_path: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct CachedProductState {
    #[serde(default, alias = "baseProductState")]
    base_product_state: Option<BaseProductState>,
    #[serde(default)]
    installed: Option<bool>,
    #[serde(default)]
    playable: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct BaseProductState {
    #[serde(default)]
    installed: Option<bool>,
    #[serde(default)]
    playable: Option<bool>,
}

fn read_product_json(fs: &impl IndexFs, path: &Path) -> Result<Vec<DiscoveredTitle>, StoreError> {
    let text = fs.read_to_string(path)?;
    let parsed: ProductFile =
        serde_json::from_str(&text).map_err(|e| StoreError::parse(path, e.to_string()))?;
    let mut out = Vec::new();
    for p in parsed.product_installs {
        let code = p.product_code.as_deref().or(p.uid.as_deref()).unwrap_or("");
        if NOT_GAMES.iter().any(|n| code.eq_ignore_ascii_case(n)) {
            continue;
        }
        let install = p
            .settings
            .as_ref()
            .and_then(|s| s.install_path.clone())
            .unwrap_or_default();
        if install.trim().is_empty() {
            continue;
        }
        let (installed, playable) = product_flags(&p.cached_product_state);
        if installed == Some(false) {
            continue;
        }
        let mut t = DiscoveredTitle::new(
            StoreId::Battlenet,
            pretty_bnet(code),
            install,
            p.uid.or(p.product_code),
        );
        t.installed = installed;
        t.playable = playable;
        out.push(t);
    }
    Ok(out)
}

fn product_flags(state: &Option<CachedProductState>) -> (Option<bool>, Option<bool>) {
    let Some(s) = state else {
        return (None, None);
    };
    let installed = s
        .installed
        .or_else(|| s.base_product_state.as_ref().and_then(|b| b.installed));
    let playable = s
        .playable
        .or_else(|| s.base_product_state.as_ref().and_then(|b| b.playable));
    (installed, playable)
}

fn pretty_bnet(code: &str) -> String {
    match code.to_ascii_lowercase().as_str() {
        "wow" => "World of Warcraft".into(),
        "d3" => "Diablo III".into(),
        "hs" => "Hearthstone".into(),
        "os" => "Overwatch".into(),
        "s2" => "StarCraft II".into(),
        "prometheus" | "pro" => "Overwatch".into(),
        "fenris" => "Diablo IV".into(),
        other if other.is_empty() => "Battle.net title".into(),
        other => other.to_string(),
    }
}
