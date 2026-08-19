use std::path::{Path, PathBuf};

use crate::error::StoreWarning;
use crate::fs::{is_ubisoft_ownership_path, IndexFs};
use crate::model::{DiscoveredTitle, StoreId};
use crate::registry::{first_value, RegistryHive};

pub const UBI_INSTALLS_WOW64: &str = r"SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs";
pub const UBI_INSTALLS: &str = r"SOFTWARE\Ubisoft\Launcher\Installs";

/// Ubisoft Connect / Uplay official game list: registry `Installs\{id}`
/// (InstallDir + Language).
///
/// **Do not** read `configurations/ownership` (or `cache/ownership`).
pub fn discover(
    hive: &impl RegistryHive,
    fs: &impl IndexFs,
    json_index: Option<&Path>,
) -> (Vec<DiscoveredTitle>, Vec<StoreWarning>) {
    let mut titles = Vec::new();
    let mut warnings = Vec::new();
    let mut seen = std::collections::BTreeSet::new();

    for root in [UBI_INSTALLS_WOW64, UBI_INSTALLS] {
        let subkeys = match hive.list_subkeys(root) {
            Ok(k) => k,
            Err(err) if err.is_not_found() => continue,
            Err(err) => {
                warnings.push(StoreWarning::new(StoreId::Ubisoft, err.to_string()));
                continue;
            }
        };
        for id in subkeys {
            let key = format!("{root}\\{id}");
            let Some(dir) = first_value(hive, &key, &["InstallDir", "installdir", "InstallPath"])
            else {
                continue;
            };
            let language = first_value(hive, &key, &["Language", "language"]);
            let folder = Path::new(&dir)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| id.clone());
            if seen.insert(id.clone()) {
                let mut title =
                    DiscoveredTitle::new(StoreId::Ubisoft, folder, PathBuf::from(dir), Some(id));
                title.language = language;
                titles.push(title);
            }
        }
    }

    if let Some(index) = json_index {
        if is_ubisoft_ownership_path(index) {
            warnings.push(StoreWarning::new(
                StoreId::Ubisoft,
                format!("refusing Ubisoft ownership path {}", index.display()),
            ));
            return (titles, warnings);
        }
        // Optional sidecar is allowed; ownership paths are not.
        match read_json_index(fs, index) {
            Ok(extra) => {
                for t in extra {
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
            Err(err) => warnings.push(StoreWarning::new(StoreId::Ubisoft, err.to_string())),
        }
    }

    (titles, warnings)
}

fn read_json_index(
    fs: &impl IndexFs,
    path: &Path,
) -> Result<Vec<DiscoveredTitle>, crate::error::StoreError> {
    if is_ubisoft_ownership_path(path) {
        return Err(crate::error::StoreError::forbidden(
            path,
            "Ubisoft configurations/ownership must never be read",
        ));
    }
    if !fs.exists(path) {
        return Err(crate::error::StoreError::io(
            path,
            std::io::Error::new(std::io::ErrorKind::NotFound, "ubisoft index missing"),
        ));
    }
    let text = fs.read_to_string(path)?;
    let idx: UbiIndex = serde_json::from_str(&text)
        .map_err(|e| crate::error::StoreError::parse(path, e.to_string()))?;
    let mut out = Vec::new();
    for g in idx.games {
        let install = g.install_dir.or(g.install_path).unwrap_or_default();
        if install.trim().is_empty() {
            continue;
        }
        let title = g.title.or(g.name).unwrap_or_else(|| {
            Path::new(&install)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        let mut t = DiscoveredTitle::new(StoreId::Ubisoft, title, install, g.id);
        t.language = g.language;
        out.push(t);
    }
    Ok(out)
}

#[derive(Debug, serde::Deserialize)]
struct UbiIndex {
    #[serde(default)]
    games: Vec<UbiGame>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct UbiGame {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    install_dir: Option<String>,
    #[serde(default)]
    install_path: Option<String>,
    #[serde(default)]
    language: Option<String>,
}
