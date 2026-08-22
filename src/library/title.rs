//! Unified library title: Steam (existing scan) plus extra launchers.

use std::path::{Path, PathBuf};

use stores::{DiscoveredTitle, StoreId};

use super::steam::{cheap_install_sizes, sizes_indicate_compacted, SteamGame};

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
    /// Same-scope WOF probe (sampled files). Not inferred from Steam `SizeOnDisk`
    /// vs a shallow folder listing.
    pub compacted: bool,
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
    /// User-picked folder. Not a launcher index.
    Custom,
}

impl LibraryStore {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Steam => "steam",
            Self::Extra(id) => id.as_str(),
            Self::Custom => "custom",
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
            Self::Custom => "Custom",
        }
    }

    /// Poster glyph for this launcher (`assets/icons/store-*.svg`, or `folder.svg` for Custom).
    ///
    /// `None` means the title has no launcher to identify — omit the badge.
    pub fn icon_path(self) -> Option<&'static str> {
        Some(match self {
            Self::Steam => "icons/store-steam.svg",
            Self::Extra(StoreId::Epic) => "icons/store-epic.svg",
            Self::Extra(StoreId::Gog) => "icons/store-gog.svg",
            Self::Extra(StoreId::Ea) => "icons/store-ea.svg",
            Self::Extra(StoreId::Ubisoft) => "icons/store-ubisoft.svg",
            Self::Extra(StoreId::Riot) => "icons/store-riot.svg",
            Self::Extra(StoreId::Battlenet) => "icons/store-battlenet.svg",
            Self::Extra(StoreId::Itch) => "icons/store-itch.svg",
            Self::Extra(StoreId::XboxGames) => "icons/store-xbox.svg",
            Self::Custom => "icons/folder.svg",
        })
    }

    pub fn is_steam(self) -> bool {
        matches!(self, Self::Steam)
    }

    /// Steam can launch via protocol. Other stores open the install folder.
    pub fn launch_label(self) -> &'static str {
        if self.is_steam() {
            "Play"
        } else {
            "Open folder"
        }
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

pub fn custom_title_id(path: &Path) -> String {
    format!("custom:{}", normalize_path_key(path))
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
            compacted: game.compacted,
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
            compacted: sizes_indicate_compacted(on_disk, logical),
            steam_app_id: None,
            steam_library_path: None,
            steam_install_dir_name: None,
            cover_url: None,
        }
    }

    /// One user-picked install folder. `None` if the path is missing or not a directory.
    pub fn from_custom_directory(path: PathBuf) -> Option<Self> {
        if !path.is_dir() {
            return None;
        }
        let name = path.file_name()?.to_string_lossy().trim().to_string();
        if name.is_empty() {
            return None;
        }
        let (logical, on_disk) = cheap_install_sizes(&path);
        Some(Self {
            id: custom_title_id(&path),
            name,
            install_path: path,
            store: LibraryStore::Custom,
            launcher_id: None,
            last_played_unix: None,
            logical_bytes: logical,
            on_disk_bytes: on_disk,
            compacted: sizes_indicate_compacted(on_disk, logical),
            steam_app_id: None,
            steam_library_path: None,
            steam_install_dir_name: None,
            cover_url: None,
        })
    }

    pub fn steam_app_id(&self) -> Option<u32> {
        self.steam_app_id
    }

    pub fn is_compacted(&self) -> bool {
        self.compacted
    }

    pub fn saved_bytes(&self) -> Option<u64> {
        let logical = self.logical_bytes?;
        let disk = self.on_disk_bytes?;
        (disk < logical).then_some(logical - disk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::steam::sizes_indicate_compacted;
    use std::path::{Path, PathBuf};
    use stores::StoreId;

    fn title(compacted: bool, logical: Option<u64>, on_disk: Option<u64>) -> LibraryTitle {
        LibraryTitle {
            id: "steam:1".into(),
            name: "Test".into(),
            install_path: PathBuf::from(r"D:\Steam\steamapps\common\Test"),
            store: LibraryStore::Steam,
            launcher_id: Some("1".into()),
            last_played_unix: None,
            logical_bytes: logical,
            on_disk_bytes: on_disk,
            compacted,
            steam_app_id: Some(1),
            steam_library_path: None,
            steam_install_dir_name: None,
            cover_url: None,
        }
    }

    #[test]
    fn catalog_vs_shallow_sizes_do_not_mark_compacted() {
        let game = title(false, Some(40_000_000_000), Some(4_096));
        assert!(
            !game.is_compacted(),
            "SizeOnDisk vs a tiny root file must not count as compacted"
        );
        assert!(sizes_indicate_compacted(
            game.on_disk_bytes,
            game.logical_bytes
        ));
    }

    #[test]
    fn probe_flag_is_the_compacted_source() {
        assert!(title(true, Some(20), Some(20)).is_compacted());
        assert!(!title(false, Some(10), Some(4)).is_compacted());
    }

    #[test]
    fn known_launchers_ship_store_icons() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
        for store in [
            LibraryStore::Steam,
            LibraryStore::Extra(StoreId::Epic),
            LibraryStore::Extra(StoreId::Gog),
            LibraryStore::Extra(StoreId::Ea),
            LibraryStore::Extra(StoreId::Ubisoft),
            LibraryStore::Extra(StoreId::Riot),
            LibraryStore::Extra(StoreId::Battlenet),
            LibraryStore::Extra(StoreId::Itch),
            LibraryStore::Extra(StoreId::XboxGames),
            LibraryStore::Custom,
        ] {
            let rel = store
                .icon_path()
                .unwrap_or_else(|| panic!("{} should have a launcher icon", store.badge()));
            let bytes = std::fs::read(root.join(rel))
                .unwrap_or_else(|err| panic!("missing {rel} for {}: {err}", store.badge()));
            let text = String::from_utf8(bytes).expect("store icon is utf-8 svg");
            assert!(text.contains("<svg"), "{rel} should be an svg");
            assert!(
                text.contains("currentColor"),
                "{rel} should tint via currentColor"
            );
        }
    }

    #[test]
    fn custom_title_id_is_stable_across_slash_styles() {
        assert_eq!(
            custom_title_id(Path::new(r"D:\Games\Hades")),
            custom_title_id(Path::new(r"D:/Games/Hades/"))
        );
        assert_eq!(
            custom_title_id(Path::new(r"D:\Games\Hades")),
            "custom:d:\\games\\hades"
        );
        assert_eq!(LibraryStore::Custom.badge(), "Custom");
        assert_eq!(LibraryStore::Custom.as_str(), "custom");
        assert_eq!(LibraryStore::Custom.icon_path(), Some("icons/folder.svg"));
    }

    #[test]
    fn steam_launch_plays_other_stores_open_the_folder() {
        assert_eq!(LibraryStore::Steam.launch_label(), "Play");
        assert_eq!(LibraryStore::Custom.launch_label(), "Open folder");
        assert_eq!(
            LibraryStore::Extra(StoreId::Epic).launch_label(),
            "Open folder"
        );
    }

    #[test]
    fn saved_bytes_only_when_disk_is_below_logical() {
        assert_eq!(title(true, Some(100), Some(40)).saved_bytes(), Some(60));
        assert_eq!(title(false, Some(100), Some(100)).saved_bytes(), None);
        assert_eq!(title(false, Some(100), None).saved_bytes(), None);
        assert_eq!(title(false, None, Some(40)).saved_bytes(), None);
    }
}
