use std::path::Path;

use serde::Deserialize;

use crate::error::{StoreError, StoreWarning};
use crate::fs::IndexFs;
use crate::model::{DiscoveredTitle, StoreId};

/// Agent / product codes that are the launcher itself, not games.
const NOT_GAMES: &[&str] = &["agent", "bna", "battle.net", "catalogs", "bts"];

/// Battle.net Agent product install records. We read JSON indexes only
/// (`product_installs.json`, `products.json`, or `*.json` catalogs).
/// Binary `product.db` protobuf is not parsed here (fixture-friendly).
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
    #[serde(default)]
    products: Vec<ProductInstall>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ProductInstall {
    #[serde(default)]
    uid: Option<String>,
    #[serde(default)]
    product_code: Option<String>,
    #[serde(default)]
    install_path: Option<String>,
    #[serde(default)]
    title: Option<String>,
}

fn read_product_json(fs: &impl IndexFs, path: &Path) -> Result<Vec<DiscoveredTitle>, StoreError> {
    let text = fs.read_to_string(path)?;
    let parsed: ProductFile =
        serde_json::from_str(&text).map_err(|e| StoreError::parse(path, e.to_string()))?;
    let mut out = Vec::new();
    for p in parsed
        .product_installs
        .into_iter()
        .chain(parsed.products)
    {
        let code = p
            .product_code
            .as_deref()
            .or(p.uid.as_deref())
            .unwrap_or("");
        if NOT_GAMES.iter().any(|n| code.eq_ignore_ascii_case(n)) {
            continue;
        }
        let install = p.install_path.unwrap_or_default();
        if install.trim().is_empty() {
            continue;
        }
        let title = p.title.unwrap_or_else(|| pretty_bnet(code));
        out.push(DiscoveredTitle {
            store: StoreId::Battlenet,
            title,
            install_path: install.into(),
            launcher_id: p.uid.or(p.product_code),
        });
    }
    Ok(out)
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
