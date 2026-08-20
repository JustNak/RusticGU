use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use crate::error::{StoreError, StoreResult, StoreWarning};
use crate::fs::IndexFs;
use crate::model::{DiscoveredTitle, StoreId};
use crate::util::{parse_query, query_value};

/// Origin `ProgramData/Origin/LocalContent/**/*.mfst` plus an optional EA
/// Desktop JSON install catalog. No volume walk.
pub fn discover(
    fs: &impl IndexFs,
    origin_local_content: Option<&Path>,
    ea_index: Option<&Path>,
) -> (Vec<DiscoveredTitle>, Vec<StoreWarning>) {
    let mut titles = Vec::new();
    let mut warnings = Vec::new();
    if let Some(dir) = origin_local_content {
        let (t, w) = discover_origin(fs, dir);
        titles.extend(t);
        warnings.extend(w);
    }
    if let Some(index) = ea_index {
        match discover_ea_index(fs, index) {
            Ok(t) => titles.extend(t),
            Err(err) if err.is_not_found() => {}
            Err(err) => warnings.push(StoreWarning::new(StoreId::Ea, err.to_string())),
        }
    }
    (titles, warnings)
}

fn discover_origin(fs: &impl IndexFs, dir: &Path) -> (Vec<DiscoveredTitle>, Vec<StoreWarning>) {
    let mut titles = Vec::new();
    let mut warnings = Vec::new();
    if !fs.is_dir(dir) {
        return (titles, warnings);
    }
    let games = match fs.list_dir(dir) {
        Ok(e) => e,
        Err(err) => {
            warnings.push(StoreWarning::new(StoreId::Ea, err.to_string()));
            return (titles, warnings);
        }
    };
    for game_dir in games {
        if !game_dir.is_dir {
            if game_dir.name.to_ascii_lowercase().ends_with(".mfst") {
                match parse_mfst(fs, &game_dir.path) {
                    Ok(Some(t)) => titles.push(t),
                    Ok(None) => {}
                    Err(err) => warnings.push(StoreWarning::new(StoreId::Ea, err.to_string())),
                }
            }
            continue;
        }
        let files = match fs.list_dir(&game_dir.path) {
            Ok(e) => e,
            Err(err) => {
                warnings.push(StoreWarning::new(StoreId::Ea, err.to_string()));
                continue;
            }
        };
        for f in files {
            if f.is_dir || !f.name.to_ascii_lowercase().ends_with(".mfst") {
                continue;
            }
            match parse_mfst(fs, &f.path) {
                Ok(Some(t)) => titles.push(t),
                Ok(None) => {}
                Err(err) => warnings.push(StoreWarning::new(StoreId::Ea, err.to_string())),
            }
        }
    }
    (titles, warnings)
}

fn parse_mfst(fs: &impl IndexFs, path: &Path) -> StoreResult<Option<DiscoveredTitle>> {
    let text = fs.read_to_string(path)?;
    let pairs = parse_query(text.trim());
    let install = query_value(&pairs, "dipInstallPath")
        .or_else(|| query_value(&pairs, "dipinstallpath"))
        .or_else(|| query_value(&pairs, "installPath"));
    let Some(install) = install.filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    // Steam-bridged Origin titles end with @steam, but they are still a valid EA index entry.
    let id = query_value(&pairs, "id").map(str::to_string);
    let title = query_value(&pairs, "displayName")
        .or_else(|| query_value(&pairs, "displayNameLoc"))
        .map(str::to_string)
        .or_else(|| {
            path.parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "Unknown EA title".into());
    Ok(Some(DiscoveredTitle::new(StoreId::Ea, title, install, id)))
}

#[derive(Debug, Deserialize)]
struct EaIndexFile {
    #[serde(default)]
    games: Vec<EaGame>,
    #[serde(default)]
    installs: Vec<EaGame>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct EaGame {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    install_path: Option<String>,
    #[serde(default)]
    base_install_path: Option<String>,
    #[serde(default)]
    base_slug: Option<String>,
    #[serde(default)]
    content_id: Option<String>,
    #[serde(default)]
    software_id: Option<String>,
}

fn discover_ea_index(fs: &impl IndexFs, path: &Path) -> StoreResult<Vec<DiscoveredTitle>> {
    if !fs.exists(path) {
        return Err(StoreError::io(
            path,
            std::io::Error::new(std::io::ErrorKind::NotFound, "ea index missing"),
        ));
    }
    let text = fs.read_to_string(path)?;
    let value: Value =
        serde_json::from_str(&text).map_err(|e| StoreError::parse(path, e.to_string()))?;
    let parsed: EaIndexFile =
        serde_json::from_value(value).map_err(|e| StoreError::parse(path, e.to_string()))?;
    let mut out = Vec::new();
    for g in parsed.games.into_iter().chain(parsed.installs) {
        let install = g.install_path.or(g.base_install_path).unwrap_or_default();
        if install.trim().is_empty() {
            continue;
        }
        let title = g
            .title
            .or(g.display_name)
            .or(g.base_slug)
            .unwrap_or_else(|| "Unknown EA title".into());
        let launcher_id = g.software_id.or(g.content_id);
        out.push(DiscoveredTitle::new(
            StoreId::Ea,
            title,
            install,
            launcher_id,
        ));
    }
    Ok(out)
}
