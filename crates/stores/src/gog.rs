use std::path::PathBuf;

use crate::error::StoreWarning;
use crate::model::{DiscoveredTitle, StoreId};
use crate::registry::{first_value, RegistryHive};

/// Official GOG Galaxy / GOG.com game keys (32-bit and 64-bit views).
pub const GOG_GAMES_WOW64: &str = r"SOFTWARE\WOW6432Node\GOG.com\Games";
pub const GOG_GAMES: &str = r"SOFTWARE\GOG.com\Games";

/// Read PATH / name / id from the GOG Games hive. Missing hive → empty list.
pub fn discover(hive: &impl RegistryHive) -> (Vec<DiscoveredTitle>, Vec<StoreWarning>) {
    let mut titles = Vec::new();
    let mut warnings = Vec::new();
    let mut seen_ids = std::collections::BTreeSet::new();
    for root in [GOG_GAMES_WOW64, GOG_GAMES] {
        let subkeys = match hive.list_subkeys(root) {
            Ok(keys) => keys,
            Err(err) if err.is_not_found() => continue,
            Err(err) => {
                warnings.push(StoreWarning::new(StoreId::Gog, err.to_string()));
                continue;
            }
        };
        for id in subkeys {
            let key = format!("{root}\\{id}");
            match read_game(hive, &key, &id) {
                Some(title) => {
                    let dedupe = title
                        .launcher_id
                        .clone()
                        .unwrap_or_else(|| title.title.clone());
                    if seen_ids.insert(dedupe) {
                        titles.push(title);
                    }
                }
                None => {}
            }
        }
    }
    (titles, warnings)
}

fn read_game(hive: &impl RegistryHive, key: &str, fallback_id: &str) -> Option<DiscoveredTitle> {
    let path = first_value(hive, key, &["PATH", "path", "workingDir", "workingdir"])?;
    let name = first_value(
        hive,
        key,
        &["gameName", "GAMENAME", "name", "startMenu", "productName"],
    )
    .unwrap_or_else(|| fallback_id.to_string());
    let launcher_id = first_value(hive, key, &["gameID", "gameId", "productID", "productId"])
        .or_else(|| Some(fallback_id.to_string()));
    Some(DiscoveredTitle {
        store: StoreId::Gog,
        title: name,
        install_path: PathBuf::from(path),
        launcher_id,
    })
}
