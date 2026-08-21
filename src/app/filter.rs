//! Main-window view filter (library vs settings).

use gpui_component::IconName;
use stores::StoreId;

use crate::library::{LibraryStore, LibraryTitle};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilterKind {
    #[default]
    Library,
    Store(LibraryStore),
    Compacted,
    Uncompacted,
    Settings,
}

/// Sidebar tree order. A launcher is listed only when the library has titles from it.
pub const STORE_NAV_ORDER: [LibraryStore; 9] = [
    LibraryStore::Steam,
    LibraryStore::Extra(StoreId::Epic),
    LibraryStore::Extra(StoreId::Gog),
    LibraryStore::Extra(StoreId::Ea),
    LibraryStore::Extra(StoreId::Ubisoft),
    LibraryStore::Extra(StoreId::Riot),
    LibraryStore::Extra(StoreId::Battlenet),
    LibraryStore::Extra(StoreId::Itch),
    LibraryStore::Extra(StoreId::XboxGames),
];

impl FilterKind {
    pub fn nav_icon(self) -> IconName {
        match self {
            Self::Library | Self::Store(_) => IconName::Inbox,
            Self::Compacted => IconName::Folder,
            Self::Uncompacted => IconName::FolderOpen,
            Self::Settings => IconName::Settings,
        }
    }

    /// Asset path when the glyph is not a stock [`IconName`].
    pub fn nav_icon_path(self) -> Option<&'static str> {
        match self {
            Self::Library => Some("icons/gamepad.svg"),
            Self::Store(store) => store.icon_path(),
            Self::Compacted => Some("icons/file-archive.svg"),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Library => "Library",
            Self::Store(store) => store.badge(),
            Self::Compacted => "Compacted",
            Self::Uncompacted => "Uncompacted",
            Self::Settings => "Settings",
        }
    }

    pub fn shows_all_library(self) -> bool {
        matches!(self, Self::Library | Self::Settings)
    }
}

/// Launchers that currently have at least one title, in sidebar order.
pub fn store_nav_entries(games: &[LibraryTitle]) -> Vec<(LibraryStore, i32)> {
    STORE_NAV_ORDER
        .into_iter()
        .filter_map(|store| {
            let n = games.iter().filter(|g| g.store == store).count() as i32;
            (n > 0).then_some((store, n))
        })
        .collect()
}

/// Fall back to Library when `kind` is a store with no titles in `games`.
pub(crate) fn fallback_missing_store(kind: FilterKind, games: &[LibraryTitle]) -> FilterKind {
    match kind {
        FilterKind::Store(store) if !games.iter().any(|game| game.store == store) => {
            FilterKind::Library
        }
        _ => kind,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn title(store: LibraryStore, name: &str) -> LibraryTitle {
        LibraryTitle {
            id: format!("{}:{name}", store.as_str()),
            name: name.into(),
            install_path: PathBuf::from(r"D:\Games").join(name),
            store,
            launcher_id: None,
            last_played_unix: None,
            logical_bytes: None,
            on_disk_bytes: None,
            compacted: false,
            steam_app_id: None,
            steam_library_path: None,
            steam_install_dir_name: None,
            cover_url: None,
        }
    }

    #[test]
    fn store_tree_hides_launchers_without_titles() {
        let games = vec![
            title(LibraryStore::Steam, "Hades"),
            title(LibraryStore::Steam, "Celeste"),
            title(LibraryStore::Extra(StoreId::Gog), "Disco Elysium"),
        ];
        let entries = store_nav_entries(&games);
        assert_eq!(
            entries,
            vec![
                (LibraryStore::Steam, 2),
                (LibraryStore::Extra(StoreId::Gog), 1),
            ]
        );
    }

    #[test]
    fn empty_library_has_no_store_tree() {
        assert!(store_nav_entries(&[]).is_empty());
    }

    #[test]
    fn store_filter_uses_launcher_badge() {
        assert_eq!(
            FilterKind::Store(LibraryStore::Extra(StoreId::Epic)).label(),
            "Epic"
        );
        assert_eq!(
            FilterKind::Store(LibraryStore::Steam).nav_icon_path(),
            Some("icons/store-steam.svg")
        );
    }

    #[test]
    fn missing_store_filter_falls_back_to_library() {
        let games = vec![title(LibraryStore::Steam, "Hades")];
        let xbox = FilterKind::Store(LibraryStore::Extra(StoreId::XboxGames));
        assert_eq!(
            fallback_missing_store(FilterKind::Store(LibraryStore::Steam), &games),
            FilterKind::Store(LibraryStore::Steam)
        );
        assert_eq!(fallback_missing_store(xbox, &games), FilterKind::Library);
        assert_eq!(fallback_missing_store(xbox, &[]), FilterKind::Library);
        assert_eq!(
            fallback_missing_store(FilterKind::Compacted, &[]),
            FilterKind::Compacted
        );
        assert_eq!(
            fallback_missing_store(FilterKind::Settings, &[]),
            FilterKind::Settings
        );
    }

    #[test]
    fn settings_back_stack_does_not_restore_vanished_store() {
        let games = vec![title(LibraryStore::Steam, "Hades")];
        let filter = fallback_missing_store(FilterKind::Settings, &games);
        let settings_return = fallback_missing_store(
            FilterKind::Store(LibraryStore::Extra(StoreId::XboxGames)),
            &games,
        );
        assert_eq!(filter, FilterKind::Settings);
        assert_eq!(settings_return, FilterKind::Library);
    }
}
