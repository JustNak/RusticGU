use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use crate::error::StoreWarning;
use crate::fs::IndexFs;
use crate::model::{DiscoveredTitle, StoreId};
use crate::util::{parse_simple_yaml_map, yaml_value};

/// Riot Client product metadata under `ProgramData/Riot Games/Metadata`
/// plus `RiotClientInstalls.json`.
///
/// Install path: `product_install_full_path`, or leftover
/// `product_install_root` when the full path is missing.
/// `update-status.json` is **patch state**, not last-played.
pub fn discover(
    fs: &impl IndexFs,
    metadata_dir: Option<&Path>,
    installs_json: Option<&Path>,
) -> (Vec<DiscoveredTitle>, Vec<StoreWarning>) {
    let mut titles = Vec::new();
    let mut warnings = Vec::new();
    if let Some(dir) = metadata_dir {
        let (t, w) = discover_metadata(fs, dir);
        titles.extend(t);
        warnings.extend(w);
    }
    if let Some(path) = installs_json {
        match read_installs_json(fs, path) {
            Ok(t) => {
                let existing: std::collections::BTreeSet<_> = titles
                    .iter()
                    .filter_map(|x| x.launcher_id.clone())
                    .collect();
                for extra in t {
                    if extra
                        .launcher_id
                        .as_ref()
                        .is_some_and(|id| existing.contains(id))
                    {
                        continue;
                    }
                    titles.push(extra);
                }
            }
            Err(err) if err.is_not_found() => {}
            Err(err) => warnings.push(StoreWarning::new(StoreId::Riot, err.to_string())),
        }
    }
    (titles, warnings)
}

fn discover_metadata(fs: &impl IndexFs, dir: &Path) -> (Vec<DiscoveredTitle>, Vec<StoreWarning>) {
    let mut titles = Vec::new();
    let mut warnings = Vec::new();
    if !fs.is_dir(dir) {
        return (titles, warnings);
    }
    let products = match fs.list_dir(dir) {
        Ok(e) => e,
        Err(err) => {
            warnings.push(StoreWarning::new(StoreId::Riot, err.to_string()));
            return (titles, warnings);
        }
    };
    for prod in products {
        if !prod.is_dir {
            continue;
        }
        let files = match fs.list_dir(&prod.path) {
            Ok(e) => e,
            Err(err) => {
                warnings.push(StoreWarning::new(StoreId::Riot, err.to_string()));
                continue;
            }
        };
        let patch_state = files
            .iter()
            .find(|f| !f.is_dir && f.name.eq_ignore_ascii_case("update-status.json"))
            .and_then(|f| read_update_status(fs, &f.path).ok().flatten());
        for f in files {
            if f.is_dir {
                continue;
            }
            let n = f.name.to_ascii_lowercase();
            if !(n.ends_with(".yaml") || n.ends_with(".yml")) {
                continue;
            }
            match parse_product_settings(fs, &f.path, &prod.name, patch_state.clone()) {
                Ok(Some(t)) => titles.push(t),
                Ok(None) => {}
                Err(err) => warnings.push(StoreWarning::new(StoreId::Riot, err.to_string())),
            }
        }
    }
    (titles, warnings)
}

fn parse_product_settings(
    fs: &impl IndexFs,
    path: &Path,
    product_folder: &str,
    patch_state: Option<String>,
) -> crate::error::StoreResult<Option<DiscoveredTitle>> {
    let text = fs.read_to_string(path)?;
    let pairs = parse_simple_yaml_map(&text);
    let full = yaml_value(&pairs, "product_install_full_path");
    let root = yaml_value(&pairs, "product_install_root");
    let (install, leftover) = match (full, root) {
        (Some(full), _) => (full.to_string(), false),
        (None, Some(root)) => (root.to_string(), true),
        (None, None) => return Ok(None),
    };
    let title = pretty_riot_name(product_folder);
    let mut t = DiscoveredTitle::new(
        StoreId::Riot,
        title,
        install,
        Some(product_folder.to_string()),
    );
    t.leftover = leftover;
    t.patch_state = patch_state;
    Ok(Some(t))
}

fn read_update_status(fs: &impl IndexFs, path: &Path) -> crate::error::StoreResult<Option<String>> {
    let text = fs.read_to_string(path)?;
    let v: Value = serde_json::from_str(&text)
        .map_err(|e| crate::error::StoreError::parse(path, e.to_string()))?;
    let status = v
        .get("status")
        .or_else(|| v.get("state"))
        .or_else(|| v.get("patchState"))
        .or_else(|| v.get("patch_state"))
        .and_then(|x| match x {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        });
    Ok(status)
}

fn pretty_riot_name(folder: &str) -> String {
    let stem = folder.split('.').next().unwrap_or(folder);
    stem.split('_')
        .filter(|s| !s.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Deserialize)]
struct RiotInstalls {
    #[serde(default)]
    associated_client: Value,
}

fn read_installs_json(
    fs: &impl IndexFs,
    path: &Path,
) -> crate::error::StoreResult<Vec<DiscoveredTitle>> {
    if !fs.exists(path) {
        return Err(crate::error::StoreError::io(
            path,
            std::io::Error::new(std::io::ErrorKind::NotFound, "riot installs missing"),
        ));
    }
    let text = fs.read_to_string(path)?;
    let parsed: RiotInstalls = serde_json::from_str(&text)
        .map_err(|e| crate::error::StoreError::parse(path, e.to_string()))?;
    let mut out = Vec::new();
    if let Some(map) = parsed.associated_client.as_object() {
        for (game_path, _client) in map {
            if game_path.trim().is_empty() {
                continue;
            }
            let name = Path::new(game_path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| game_path.clone());
            if name.eq_ignore_ascii_case("Riot Client") {
                continue;
            }
            out.push(DiscoveredTitle::new(
                StoreId::Riot,
                name,
                game_path.as_str(),
                None,
            ));
        }
    }
    Ok(out)
}
