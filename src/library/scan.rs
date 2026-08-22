//! Merge Steam (existing scan) with `stores::discover_all`.

use std::collections::HashSet;
use std::path::PathBuf;

#[cfg(not(windows))]
use stores::EmptyHive;
use stores::{discover_all, DiscoverOptions, DiscoveredTitle, PathRoots, StdFs, StoreId};

use super::steam::{scan_steam_library, steam_path, SteamGame};
use super::title::{normalize_path_key, LibraryTitle};
use crate::settings::custom_directory_reject_reason;
use shelf::last_played_unix_from_steam_userdata;

/// Knobs for a full library scan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanOptions {
    pub include_xbox_games: bool,
    pub custom_directories: Vec<PathBuf>,
}

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
    sort_titles(&mut titles);
    titles
}

fn sort_titles(titles: &mut [LibraryTitle]) {
    titles.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
}

/// Add user-picked folders. Missing paths are skipped. Paths already in the
/// launcher library keep the launcher row.
pub fn append_custom_titles(titles: &mut Vec<LibraryTitle>, dirs: &[PathBuf]) {
    let mut occupied: HashSet<String> = titles
        .iter()
        .map(|t| normalize_path_key(&t.install_path))
        .collect();
    for dir in dirs {
        if custom_directory_reject_reason(dir).is_some() {
            continue;
        }
        let key = normalize_path_key(dir);
        if !occupied.insert(key) {
            continue;
        }
        if let Some(title) = LibraryTitle::from_custom_directory(dir.clone()) {
            titles.push(title);
        }
    }
    sort_titles(titles);
}

fn build_library(
    steam: Vec<SteamGame>,
    extra: Vec<DiscoveredTitle>,
    options: &ScanOptions,
    last_played: impl Fn(u32) -> Option<u64>,
) -> Vec<LibraryTitle> {
    let mut titles = merge_library(steam, extra, options.include_xbox_games, last_played);
    append_custom_titles(&mut titles, &options.custom_directories);
    titles
}

fn combine_scan(
    steam: Result<Vec<SteamGame>, String>,
    extra: Vec<DiscoveredTitle>,
    options: &ScanOptions,
    last_played: impl Fn(u32) -> Option<u64>,
) -> Result<Vec<LibraryTitle>, String> {
    match steam {
        Ok(steam) => Ok(build_library(steam, extra, options, last_played)),
        Err(err) => {
            let titles = build_library(Vec::new(), extra, options, last_played);
            if titles.is_empty() {
                Err(err)
            } else {
                Ok(titles)
            }
        }
    }
}

/// Steam scan + extra-store discovery + custom folders. Steam stays in `steam.rs`.
pub fn scan_library(options: ScanOptions) -> Result<Vec<LibraryTitle>, String> {
    let extra = discover_extra_titles(options.include_xbox_games);
    combine_scan(scan_steam_library(), extra, &options, steam_last_played)
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

    fn unique_temp_game(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rusticgu-custom-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn custom_titles_append_and_skip_occupied_or_missing() {
        let folder = unique_temp_game("Hades");
        let mut steam = steam_game(1, "Hades");
        steam.install_path = folder.clone();
        let mut titles = merge_library(vec![steam], Vec::new(), false, |_| None);
        append_custom_titles(&mut titles, std::slice::from_ref(&folder));
        assert_eq!(titles.len(), 1);
        assert!(titles[0].store.is_steam());

        let other = unique_temp_game("Celeste");
        append_custom_titles(
            &mut titles,
            &[
                other.clone(),
                PathBuf::from(r"Z:\rusticgu-missing-folder-does-not-exist"),
            ],
        );
        assert_eq!(
            titles
                .iter()
                .filter(|t| t.store == LibraryStore::Custom)
                .count(),
            1
        );
        let custom = titles
            .iter()
            .find(|t| t.store == LibraryStore::Custom)
            .unwrap();
        assert_eq!(custom.name, other.file_name().unwrap().to_string_lossy());
        assert!(custom.id.starts_with("custom:"));

        let _ = std::fs::remove_dir_all(&folder);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn steam_error_survives_when_custom_titles_exist() {
        let folder = unique_temp_game("Portable");
        let options = ScanOptions {
            include_xbox_games: false,
            custom_directories: vec![folder.clone()],
        };
        let titles = combine_scan(
            Err("Steam is not installed.".into()),
            Vec::new(),
            &options,
            |_| None,
        )
        .unwrap();
        assert_eq!(titles.len(), 1);
        assert_eq!(titles[0].store, LibraryStore::Custom);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn steam_error_is_fatal_when_custom_dirs_are_missing() {
        let options = ScanOptions {
            include_xbox_games: false,
            custom_directories: vec![PathBuf::from(r"Z:\rusticgu-missing-folder-does-not-exist")],
        };
        let err = combine_scan(
            Err("Steam is not installed.".into()),
            Vec::new(),
            &options,
            |_| None,
        )
        .unwrap_err();
        assert!(err.contains("Steam is not installed"));
    }
}
