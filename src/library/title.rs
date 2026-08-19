//! Unified library title: Steam (existing scan) plus extra launchers.

use std::path::{Path, PathBuf};

use stores::{DiscoveredTitle, StoreId};

use super::steam::{cheap_install_sizes, SteamGame};

/// One row in the unified library. Dual-registered titles appear twice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryTitle {
    pub id: String,
    pub name: String,
    pub install_path: PathBuf,
    pub store: LibraryStore,
    pub launcher_id: Option<String>,
    /// Steam localconfig or itch `localLastRunAt` only. Never mtime / INSTALLDATE.
    pub last_played_unix: Option<u64>,
    pub logical_bytes: Option<u64>,
    pub on_disk_bytes: Option<u64>,
    pub steam_app_id: Option<u32>,
    pub steam_library_path: Option<PathBuf>,
    pub steam_install_dir_name: Option<String>,
    /// itch `coverUrl` / `stillCoverUrl` from Fetch.Caves JSON only. Never scraped HTML.
    pub cover_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LibraryStore {
    Steam,
    Extra(StoreId),
}

impl LibraryStore {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Steam => "steam",
            Self::Extra(id) => id.as_str(),
        }
    }

    pub fn badge(self) -> &'static str {
        match self {
            Self::Steam => "Steam",
            Self::Extra(StoreId::Epic) => "Epic",
            Self::Extra(StoreId::Gog) => "GOG",
            Self::Extra(StoreId::Ea) => "EA",
            Self::Extra(StoreId::Ubisoft) => "Ubisoft",
            Self::Extra(StoreId::Riot) => "Riot",
            Self::Extra(StoreId::Battlenet) => "Battle.net",
            Self::Extra(StoreId::Itch) => "itch",
            Self::Extra(StoreId::XboxGames) => "Xbox",
        }
    }

    pub fn is_steam(self) -> bool {
        matches!(self, Self::Steam)
    }
}

impl std::fmt::Display for LibraryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.badge())
    }
}

pub fn steam_title_id(app_id: u32) -> String {
    format!("steam:{app_id}")
}

pub fn extra_title_id(title: &DiscoveredTitle) -> String {
    match title.launcher_id.as_deref() {
        Some(id) if !id.is_empty() => format!("{}:{id}", title.store.as_str()),
        _ => format!(
            "{}:{}",
            title.store.as_str(),
            normalize_path_key(&title.install_path)
        ),
    }
}

pub fn normalize_path_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

impl LibraryTitle {
    pub fn from_steam(game: SteamGame, last_played_unix: Option<u64>) -> Self {
        Self {
            id: steam_title_id(game.app_id),
            name: game.name,
            install_path: game.install_path,
            store: LibraryStore::Steam,
            launcher_id: Some(game.app_id.to_string()),
            last_played_unix,
            logical_bytes: game.logical_bytes,
            on_disk_bytes: game.on_disk_bytes,
            steam_app_id: Some(game.app_id),
            steam_library_path: Some(game.library_path),
            steam_install_dir_name: Some(game.install_dir_name),
            cover_url: None,
        }
    }

    pub fn from_discovered(title: DiscoveredTitle) -> Self {
        let (logical, on_disk) = cheap_install_sizes(&title.install_path);
        let last_played_unix = match title.store {
            StoreId::Itch => title.last_played_unix,
            _ => None,
        };
        Self {
            id: extra_title_id(&title),
            name: title.title,
            install_path: title.install_path,
            store: LibraryStore::Extra(title.store),
            launcher_id: title.launcher_id,
            last_played_unix,
            logical_bytes: logical,
            on_disk_bytes: on_disk,
            steam_app_id: None,
            steam_library_path: None,
            steam_install_dir_name: None,
            cover_url: None,
        }
    }

    pub fn steam_app_id(&self) -> Option<u32> {
        self.steam_app_id
    }

    pub fn is_compacted(&self) -> bool {
        match (self.on_disk_bytes, self.logical_bytes) {
            (Some(disk), Some(logical)) => disk + logical / 20 < logical,
            _ => false,
        }
    }
}
