use std::path::Path;

use serde::Deserialize;

use crate::error::{StoreResult, StoreWarning};
use crate::fs::IndexFs;
use crate::model::{DiscoveredTitle, StoreId};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct EpicManifest {
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    install_location: Option<String>,
    #[serde(default)]
    app_name: Option<String>,
    #[serde(default)]
    catalog_item_id: Option<String>,
    #[serde(default, rename = "bIsIncompleteInstall")]
    is_incomplete: Option<bool>,
}

/// Parse Epic Games Launcher `*.item` / `*.json` manifests in `Manifests`.
/// Does not walk volumes. It only reads the official manifest directory.
pub fn discover(
    fs: &impl IndexFs,
    manifests_dir: &Path,
) -> (Vec<DiscoveredTitle>, Vec<StoreWarning>) {
    let mut titles = Vec::new();
    let mut warnings = Vec::new();
    if !fs.is_dir(manifests_dir) {
        return (titles, warnings);
    }
    let entries = match fs.list_dir(manifests_dir) {
        Ok(e) => e,
        Err(err) => {
            warnings.push(StoreWarning::new(StoreId::Epic, err.to_string()));
            return (titles, warnings);
        }
    };
    for ent in entries {
        if ent.is_dir {
            continue;
        }
        let name = ent.name.to_ascii_lowercase();
        if !(name.ends_with(".item") || name.ends_with(".json")) {
            continue;
        }
        match parse_manifest(fs, &ent.path) {
            Ok(Some(title)) => titles.push(title),
            Ok(None) => {}
            Err(err) => warnings.push(StoreWarning::new(StoreId::Epic, err.to_string())),
        }
    }
    (titles, warnings)
}

fn parse_manifest(fs: &impl IndexFs, path: &Path) -> StoreResult<Option<DiscoveredTitle>> {
    let text = fs.read_to_string(path)?;
    let m: EpicManifest = serde_json::from_str(&text)
        .map_err(|e| crate::error::StoreError::parse(path, e.to_string()))?;
    if m.is_incomplete == Some(true) {
        return Ok(None);
    }
    let title = m.display_name.unwrap_or_default();
    let install = m.install_location.unwrap_or_default();
    if title.trim().is_empty() || install.trim().is_empty() {
        return Ok(None);
    }
    let launcher_id = m
        .catalog_item_id
        .filter(|s| !s.is_empty())
        .or(m.app_name.filter(|s| !s.is_empty()));
    Ok(Some(DiscoveredTitle::new(
        StoreId::Epic,
        title.trim(),
        install,
        launcher_id,
    )))
}
