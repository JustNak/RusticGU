use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::error::{StoreError, StoreWarning};
use crate::fs::IndexFs;
use crate::model::{DiscoveredTitle, StoreId};
use crate::util::file_name_eq_ignore_case;

/// itch.io app / install indexes only.
///
/// **Never** opens `butler.db` (it contains logins). Discovery reads:
/// - `preferences.json` `installLocations`
/// - `library.json` / `caves.json` / `index.json` cave lists
/// - `.itch/receipt.json` one level under each *indexed* install location
///
/// Install-location children are listed at depth 1 only (the launcher's own
/// library roots), never a volume walk.
pub fn discover(fs: &impl IndexFs, config_dir: &Path) -> (Vec<DiscoveredTitle>, Vec<StoreWarning>) {
    let mut titles = Vec::new();
    let mut warnings = Vec::new();
    if !fs.exists(config_dir) {
        return (titles, warnings);
    }

    let mut seen = std::collections::BTreeSet::new();
    let mut push = |t: DiscoveredTitle| {
        let key = t
            .install_path
            .to_string_lossy()
            .replace('\\', "/")
            .to_ascii_lowercase();
        if seen.insert(key) {
            titles.push(t);
        }
    };

    for name in ["library.json", "caves.json", "index.json", "apps.json"] {
        let path = config_dir.join(name);
        match read_cave_index(fs, &path) {
            Ok(found) => {
                for t in found {
                    push(t);
                }
            }
            Err(err) if err.is_not_found() => {}
            Err(err) => warnings.push(StoreWarning::new(StoreId::Itch, err.to_string())),
        }
    }

    let prefs = config_dir.join("preferences.json");
    match read_preferences_locations(fs, &prefs) {
        Ok(locations) => {
            for loc in locations {
                let (found, w) = receipts_in_location(fs, &loc);
                warnings.extend(w);
                for t in found {
                    push(t);
                }
            }
        }
        Err(err) if err.is_not_found() => {}
        Err(err) => warnings.push(StoreWarning::new(StoreId::Itch, err.to_string())),
    }

    (titles, warnings)
}

#[derive(Debug, Deserialize)]
struct CaveFile {
    #[serde(default)]
    caves: Vec<Cave>,
    #[serde(default)]
    apps: Vec<Cave>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Cave {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    cave_id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    install_path: Option<String>,
    #[serde(default)]
    install_folder: Option<String>,
    #[serde(default)]
    game: Option<CaveGame>,
}

#[derive(Debug, Deserialize, Default)]
struct CaveGame {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    id: Option<Value>,
}

fn read_cave_index(fs: &impl IndexFs, path: &Path) -> Result<Vec<DiscoveredTitle>, StoreError> {
    if file_name_eq_ignore_case(path, "butler.db") {
        return Err(StoreError::forbidden(
            path,
            "itch butler.db contains logins and must never be opened",
        ));
    }
    if !fs.exists(path) {
        return Err(StoreError::io(
            path,
            std::io::Error::new(std::io::ErrorKind::NotFound, "itch index missing"),
        ));
    }
    let text = fs.read_to_string(path)?;
    let parsed: CaveFile =
        serde_json::from_str(&text).map_err(|e| StoreError::parse(path, e.to_string()))?;
    let mut out = Vec::new();
    for c in parsed.caves.into_iter().chain(parsed.apps) {
        if let Some(t) = cave_to_title(c) {
            out.push(t);
        }
    }
    Ok(out)
}

fn cave_to_title(c: Cave) -> Option<DiscoveredTitle> {
    let install = c
        .install_path
        .or(c.install_folder)
        .filter(|s| !s.trim().is_empty())?;
    let title = c
        .title
        .or_else(|| c.game.as_ref().and_then(|g| g.title.clone()))
        .or_else(|| {
            Path::new(&install)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })?;
    let launcher_id = c.id.or(c.cave_id).or_else(|| {
        c.game.and_then(|g| match g.id {
            Some(Value::Number(n)) => Some(n.to_string()),
            Some(Value::String(s)) => Some(s),
            _ => None,
        })
    });
    Some(DiscoveredTitle {
        store: StoreId::Itch,
        title,
        install_path: install.into(),
        launcher_id,
    })
}

fn read_preferences_locations(fs: &impl IndexFs, path: &Path) -> Result<Vec<PathBuf>, StoreError> {
    if !fs.exists(path) {
        return Err(StoreError::io(
            path,
            std::io::Error::new(std::io::ErrorKind::NotFound, "preferences missing"),
        ));
    }
    let text = fs.read_to_string(path)?;
    let v: Value =
        serde_json::from_str(&text).map_err(|e| StoreError::parse(path, e.to_string()))?;
    let mut out = Vec::new();
    if let Some(locs) = v.get("installLocations") {
        match locs {
            Value::Object(map) => {
                for (_id, entry) in map {
                    if let Some(p) = location_path(entry) {
                        out.push(p);
                    }
                }
            }
            Value::Array(arr) => {
                for entry in arr {
                    if let Some(p) = location_path(entry) {
                        out.push(p);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

fn location_path(entry: &Value) -> Option<PathBuf> {
    match entry {
        Value::String(s) if !s.trim().is_empty() => Some(PathBuf::from(s)),
        Value::Object(map) => map
            .get("path")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from),
        _ => None,
    }
}

fn receipts_in_location(fs: &impl IndexFs, loc: &Path) -> (Vec<DiscoveredTitle>, Vec<StoreWarning>) {
    let mut titles = Vec::new();
    let mut warnings = Vec::new();
    if crate::util::looks_like_volume_root(loc) {
        warnings.push(StoreWarning::new(
            StoreId::Itch,
            format!("refusing to walk volume root {}", loc.display()),
        ));
        return (titles, warnings);
    }
    if !fs.is_dir(loc) {
        return (titles, warnings);
    }
    let children = match fs.list_dir(loc) {
        Ok(e) => e,
        Err(err) => {
            warnings.push(StoreWarning::new(StoreId::Itch, err.to_string()));
            return (titles, warnings);
        }
    };
    for child in children {
        if !child.is_dir {
            continue;
        }
        let receipt = child.path.join(".itch").join("receipt.json");
        match read_receipt(fs, &receipt, &child.path) {
            Ok(Some(t)) => titles.push(t),
            Ok(None) => {}
            Err(err) if err.is_not_found() => {}
            Err(err) => warnings.push(StoreWarning::new(StoreId::Itch, err.to_string())),
        }
    }
    (titles, warnings)
}

fn read_receipt(
    fs: &impl IndexFs,
    path: &Path,
    install: &Path,
) -> Result<Option<DiscoveredTitle>, StoreError> {
    if !fs.exists(path) {
        return Err(StoreError::io(
            path,
            std::io::Error::new(std::io::ErrorKind::NotFound, "receipt missing"),
        ));
    }
    let text = fs.read_to_string(path)?;
    let v: Value =
        serde_json::from_str(&text).map_err(|e| StoreError::parse(path, e.to_string()))?;
    let title = v
        .get("game")
        .and_then(|g| g.get("title"))
        .and_then(Value::as_str)
        .or_else(|| v.get("title").and_then(Value::as_str))
        .map(str::to_string)
        .or_else(|| {
            install
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        });
    let Some(title) = title else {
        return Ok(None);
    };
    let launcher_id = v
        .get("caveId")
        .or_else(|| v.get("cave_id"))
        .and_then(|x| match x {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .or_else(|| {
            v.get("game")
                .and_then(|g| g.get("id"))
                .and_then(|id| match id {
                    Value::String(s) => Some(s.clone()),
                    Value::Number(n) => Some(n.to_string()),
                    _ => None,
                })
        });
    Ok(Some(DiscoveredTitle {
        store: StoreId::Itch,
        title,
        install_path: install.to_path_buf(),
        launcher_id,
    }))
}
