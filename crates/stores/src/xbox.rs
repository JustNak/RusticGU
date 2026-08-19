use std::path::Path;

use crate::error::StoreWarning;
use crate::fs::IndexFs;
use crate::model::{DiscoveredTitle, StoreId};
use crate::util::{extract_xml_value, looks_like_volume_root, path_contains_component};

/// GDK / XboxGames discovery. **Opt-in only.** Caller must pass explicit
/// library roots (never a drive letter). WindowsApps is refused.
pub fn discover(
    fs: &impl IndexFs,
    roots: &[std::path::PathBuf],
) -> (Vec<DiscoveredTitle>, Vec<StoreWarning>) {
    let mut titles = Vec::new();
    let mut warnings = Vec::new();
    for root in roots {
        if looks_like_volume_root(root) {
            warnings.push(StoreWarning::new(
                StoreId::XboxGames,
                format!("refusing XboxGames volume root {}", root.display()),
            ));
            continue;
        }
        if path_contains_component(root, "WindowsApps") {
            warnings.push(StoreWarning::new(
                StoreId::XboxGames,
                format!("refusing WindowsApps path {}", root.display()),
            ));
            continue;
        }
        if !fs.is_dir(root) {
            continue;
        }
        let kids = match fs.list_dir(root) {
            Ok(e) => e,
            Err(err) => {
                warnings.push(StoreWarning::new(StoreId::XboxGames, err.to_string()));
                continue;
            }
        };
        for game in kids {
            if !game.is_dir {
                continue;
            }
            match read_game(fs, &game.path, &game.name) {
                Ok(Some(t)) => titles.push(t),
                Ok(None) => {}
                Err(err) => warnings.push(StoreWarning::new(StoreId::XboxGames, err.to_string())),
            }
        }
    }
    (titles, warnings)
}

fn read_game(
    fs: &impl IndexFs,
    dir: &Path,
    folder_name: &str,
) -> crate::error::StoreResult<Option<DiscoveredTitle>> {
    let candidates = [
        dir.join("MicrosoftGame.config"),
        dir.join("Content").join("MicrosoftGame.config"),
        dir.join("appxmanifest.xml"),
        dir.join("Content").join("appxmanifest.xml"),
    ];
    for cfg in candidates {
        if !fs.exists(&cfg) {
            continue;
        }
        let text = fs.read_to_string(&cfg)?;
        let title = extract_xml_value(&text, "DefaultDisplayName")
            .or_else(|| extract_xml_value(&text, "DisplayName"))
            .unwrap_or_else(|| folder_name.to_string());
        let launcher_id = extract_xml_value(&text, "Name");
        return Ok(Some(DiscoveredTitle::new(
            StoreId::XboxGames,
            title,
            dir,
            launcher_id,
        )));
    }
    Ok(None)
}
