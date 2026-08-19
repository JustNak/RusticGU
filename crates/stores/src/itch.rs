use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use crate::error::{StoreError, StoreWarning};
use crate::fs::IndexFs;
use crate::model::{DiscoveredTitle, StoreId};
use crate::util::file_name_eq_ignore_case;

/// itch.io discovery is **butlerd `Fetch.Caves` only**.
///
/// **Never** opens `butler.db` (it contains logins). We only read a
/// Fetch.Caves-shaped JSON sidecar (`fetch_caves.json` or `caves.json`).
pub fn discover(fs: &impl IndexFs, config_dir: &Path) -> (Vec<DiscoveredTitle>, Vec<StoreWarning>) {
    let mut titles = Vec::new();
    let mut warnings = Vec::new();
    if !fs.exists(config_dir) {
        return (titles, warnings);
    }

    let mut seen = std::collections::BTreeSet::new();
    for name in ["fetch_caves.json", "caves.json"] {
        let path = config_dir.join(name);
        if file_name_eq_ignore_case(&path, "butler.db") {
            continue;
        }
        match read_fetch_caves(fs, &path) {
            Ok(found) => {
                for t in found {
                    let key = t
                        .install_path
                        .to_string_lossy()
                        .replace('\\', "/")
                        .to_ascii_lowercase();
                    if seen.insert(key) {
                        titles.push(t);
                    }
                }
            }
            Err(err) if err.is_not_found() => {}
            Err(err) => warnings.push(StoreWarning::new(StoreId::Itch, err.to_string())),
        }
    }

    (titles, warnings)
}

fn read_fetch_caves(fs: &impl IndexFs, path: &Path) -> Result<Vec<DiscoveredTitle>, StoreError> {
    if file_name_eq_ignore_case(path, "butler.db") {
        return Err(StoreError::forbidden(
            path,
            "itch butler.db contains logins and must never be opened",
        ));
    }
    if !fs.exists(path) {
        return Err(StoreError::io(
            path,
            std::io::Error::new(std::io::ErrorKind::NotFound, "Fetch.Caves index missing"),
        ));
    }
    let text = fs.read_to_string(path)?;
    parse_fetch_caves(&text, path)
}

fn parse_fetch_caves(text: &str, path: &Path) -> Result<Vec<DiscoveredTitle>, StoreError> {
    let v: Value =
        serde_json::from_str(text).map_err(|e| StoreError::parse(path, e.to_string()))?;
    let mut caves: Vec<Value> = Vec::new();
    if let Some(arr) = v.get("caves").and_then(Value::as_array) {
        caves.extend(arr.iter().cloned());
    }
    if let Some(arr) = v.get("items").and_then(Value::as_array) {
        for item in arr {
            if let Some(cave) = item.get("cave") {
                caves.push(cave.clone());
            } else {
                caves.push(item.clone());
            }
        }
    }
    let parsed: FetchCaves = serde_json::from_value(Value::Object(
        [("caves".into(), Value::Array(caves))].into_iter().collect(),
    ))
    .map_err(|e| StoreError::parse(path, e.to_string()))?;
    Ok(parsed.caves.into_iter().filter_map(cave_to_title).collect())
}

#[derive(Debug, Deserialize)]
struct FetchCaves {
    #[serde(default)]
    caves: Vec<Cave>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Cave {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    game: Option<CaveGame>,
    #[serde(default)]
    install_info: Option<InstallInfo>,
    #[serde(default)]
    stats: Option<CaveStats>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    install_folder: Option<String>,
    #[serde(default)]
    install_path: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct CaveGame {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    id: Option<Value>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct InstallInfo {
    #[serde(default)]
    install_folder: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CaveStats {
    #[serde(default)]
    local_last_run_at: Option<Value>,
}

fn cave_to_title(c: Cave) -> Option<DiscoveredTitle> {
    let install = c
        .install_info
        .and_then(|i| i.install_folder)
        .or(c.install_folder)
        .or(c.install_path)
        .filter(|s| !s.trim().is_empty())?;
    let title = c
        .title
        .or_else(|| c.game.as_ref().and_then(|g| g.title.clone()))
        .or_else(|| {
            Path::new(&install)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })?;
    let launcher_id = c.id.or_else(|| {
        c.game.and_then(|g| match g.id {
            Some(Value::Number(n)) => Some(n.to_string()),
            Some(Value::String(s)) => Some(s),
            _ => None,
        })
    });
    let last_played_unix = c.stats.and_then(|s| parse_last_run(s.local_last_run_at));
    let mut t = DiscoveredTitle::new(StoreId::Itch, title, install, launcher_id);
    t.last_played_unix = last_played_unix;
    Some(t)
}

fn parse_last_run(v: Option<Value>) -> Option<u64> {
    match v? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}
