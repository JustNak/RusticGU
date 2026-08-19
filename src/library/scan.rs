//! Merge Steam (existing scan) with `stores::discover_all`.

use std::path::PathBuf;

#[cfg(not(windows))]
use stores::EmptyHive;
use stores::{discover_all, DiscoverOptions, DiscoveredTitle, PathRoots, StdFs, StoreId};

use super::steam::{scan_steam_library, steam_path, SteamGame};
use super::title::LibraryTitle;
use shelf::last_played_unix_from_steam_userdata;

/// Typical XboxGames library folders. Never a volume root, never WindowsApps.
pub fn typical_xbox_games_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        let home_xbox = home.join("XboxGames");
        if home_xbox.is_dir() && !is_windows_apps(&home_xbox) {
            roots.push(home_xbox);
        }
    }
    let c = PathBuf::from(r"C:\XboxGames");
    if c.is_dir() && !is_windows_apps(&c) {
        roots.push(c);
    }
    roots
}

fn is_windows_apps(path: &std::path::Path) -> bool {
    path.to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
        .contains("\\windowsapps\\")
}

pub fn extra_store_options(include_xbox_games: bool) -> DiscoverOptions {
    DiscoverOptions { include_xbox_games }
}

pub fn extra_store_roots(include_xbox_games: bool) -> PathRoots {
    let mut roots = PathRoots::from_env();
    if include_xbox_games {
        roots.xbox_games_roots = typical_xbox_games_roots();
    } else {
        roots.xbox_games_roots.clear();
    }
    roots
}

/// Discover extra launchers via the stores crate public API.
pub fn discover_extra_titles(include_xbox_games: bool) -> Vec<DiscoveredTitle> {
    let roots = extra_store_roots(include_xbox_games);
    let opts = extra_store_options(include_xbox_games);
    let fs = StdFs;
    #[cfg(windows)]
    {
        discover_all(&fs, &stores::registry::WindowsHive, &roots, &opts).titles
    }
    #[cfg(not(windows))]
    {
        discover_all(&fs, &EmptyHive, &roots, &opts).titles
    }
}

fn steam_last_played(app_id: u32) -> Option<u64> {
    let steam = steam_path()?;
    last_played_unix_from_steam_userdata(steam, app_id)
}

/// Merge Steam games with extra-store titles. No cross-store dedupe.
/// XboxGames rows are dropped unless `include_xbox_games` is true.
pub fn merge_library(
    steam: Vec<SteamGame>,
    extra: Vec<DiscoveredTitle>,
    include_xbox_games: bool,
    last_played: impl Fn(u32) -> Option<u64>,
) -> Vec<LibraryTitle> {
    let mut titles = Vec::with_capacity(steam.len() + extra.len());
    for game in steam {
        let played = last_played(game.app_id);
        titles.push(LibraryTitle::from_steam(game, played));
    }
    for discovered in extra {
        if discovered.store == StoreId::XboxGames && !include_xbox_games {
            continue;
        }
        titles.push(LibraryTitle::from_discovered(discovered));
    }
    titles.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    titles
}

/// Steam scan + extra-store discovery. Steam stays in `steam.rs`.
pub fn scan_library(include_xbox_games: bool) -> Result<Vec<LibraryTitle>, String> {
    let extra = discover_extra_titles(include_xbox_games);
    match scan_steam_library() {
        Ok(steam) => Ok(merge_library(
            steam,
            extra,
            include_xbox_games,
            steam_last_played,
        )),
        Err(err) => {
            if extra.is_empty() {
                Err(err)
            } else {
                Ok(merge_library(
                    Vec::new(),
                    extra,
                    include_xbox_games,
                    steam_last_played,
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::LibraryStore;
    use std::path::PathBuf;
    use stores::DiscoveredTitle;

    fn steam_game(app_id: u32, name: &str) -> SteamGame {
        SteamGame {
            app_id,
            name: name.into(),
            install_dir_name: name.into(),
            install_path: PathBuf::from(format!(r"D:\Steam\steamapps\common\{name}")),
            library_path: PathBuf::from(r"D:\Steam"),
            logical_bytes: Some(1_000),
            on_disk_bytes: Some(1_000),
            compacted: false,
        }
    }

    #[test]
    fn extra_store_titles_appear_and_steam_stays() {
        let steam = vec![steam_game(730, "Counter-Strike 2")];
        let extra = vec![
            DiscoveredTitle::new(
                StoreId::Epic,
                "Hades",
                r"D:\Epic\Hades",
                Some("hades".into()),
            ),
            DiscoveredTitle::new(
                StoreId::Gog,
                "Hades",
                r"D:\GOG\Hades",
                Some("1207659012".into()),
            ),
        ];
        let titles = merge_library(steam, extra, false, |_| None);
        assert!(titles
            .iter()
            .any(|t| t.store.is_steam() && t.steam_app_id == Some(730)));
        assert!(titles
            .iter()
            .any(|t| { t.store == LibraryStore::Extra(StoreId::Epic) && t.name == "Hades" }));
        assert!(titles
            .iter()
            .any(|t| { t.store == LibraryStore::Extra(StoreId::Gog) && t.name == "Hades" }));
        let hades = titles.iter().filter(|t| t.name == "Hades").count();
        assert_eq!(hades, 2, "dual-register must not be deduped");
        assert!(titles.iter().any(|t| t.id == "steam:730"));
    }

    #[test]
    fn xbox_opt_in_default_off() {
        assert!(!DiscoverOptions::default().include_xbox_games);
        assert!(!extra_store_options(false).include_xbox_games);

        let steam = vec![steam_game(570, "Dota 2")];
        let extra = vec![DiscoveredTitle::new(
            StoreId::XboxGames,
            "Forza Horizon 5",
            r"C:\XboxGames\Forza",
            Some("Forza".into()),
        )];
        let off = merge_library(steam.clone(), extra.clone(), false, |_| None);
        assert!(off.iter().any(|t| t.name == "Dota 2" && t.store.is_steam()));
        assert!(off
            .iter()
            .all(|t| t.store != LibraryStore::Extra(StoreId::XboxGames)));

        let on = merge_library(steam, extra, true, |_| None);
        assert!(on
            .iter()
            .any(|t| t.store == LibraryStore::Extra(StoreId::XboxGames)));
        assert!(on.iter().any(|t| t.store.is_steam()));
    }

    #[test]
    fn last_played_not_invented_for_epic() {
        let mut epic = DiscoveredTitle::new(
            StoreId::Epic,
            "Fortnite",
            r"D:\Epic\Fortnite",
            Some("fortnite".into()),
        );
        epic.last_played_unix = Some(1_700_000_000);
        let titles = merge_library(Vec::new(), vec![epic], false, |_| None);
        assert_eq!(titles[0].last_played_unix, None);
    }

    #[test]
    fn itch_keeps_cave_stats_last_played() {
        let mut itch = DiscoveredTitle::new(
            StoreId::Itch,
            "Celeste",
            r"D:\itch\Celeste",
            Some("celeste".into()),
        );
        itch.last_played_unix = Some(1_700_000_000);
        let titles = merge_library(Vec::new(), vec![itch], false, |_| None);
        assert_eq!(titles[0].last_played_unix, Some(1_700_000_000));
    }
}
