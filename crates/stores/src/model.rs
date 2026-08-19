use std::path::PathBuf;

/// Official launcher / store identity. Steam is owned by a sibling crate
/// and is intentionally absent here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StoreId {
    Epic,
    Gog,
    Ea,
    Ubisoft,
    Riot,
    Battlenet,
    Itch,
    XboxGames,
}

impl StoreId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Epic => "epic",
            Self::Gog => "gog",
            Self::Ea => "ea",
            Self::Ubisoft => "ubisoft",
            Self::Riot => "riot",
            Self::Battlenet => "battlenet",
            Self::Itch => "itch",
            Self::XboxGames => "xboxgames",
        }
    }
}

impl std::fmt::Display for StoreId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One installed title as reported by a single launcher index.
/// Dual-registered titles (e.g. Epic + GOG) appear twice; this crate does
/// not cross-store dedupe.
///
/// `last_played_unix` is filled **only** from itch `CaveStats.localLastRunAt`.
/// Epic / GOG / Xbox / Battle.net / EA / Ubisoft / Riot stay `None` — there
/// is no safe last-play signal (do not use mtime / INSTALLDATE).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredTitle {
    pub store: StoreId,
    pub title: String,
    pub install_path: PathBuf,
    pub launcher_id: Option<String>,
    /// Ubisoft `Installs\{id}\Language`.
    pub language: Option<String>,
    /// Riot leftover: `product_install_full_path` missing, used `product_install_root`.
    pub leftover: bool,
    /// Riot `update-status.json` patch state (not last-played).
    pub patch_state: Option<String>,
    /// Battle.net `cached_product_state…installed`.
    pub installed: Option<bool>,
    /// Battle.net `cached_product_state…playable`.
    pub playable: Option<bool>,
    /// itch `CaveStats.localLastRunAt` only. All other stores: `None`.
    pub last_played_unix: Option<u64>,
}

impl DiscoveredTitle {
    pub fn new(
        store: StoreId,
        title: impl Into<String>,
        install_path: impl Into<PathBuf>,
        launcher_id: Option<String>,
    ) -> Self {
        Self {
            store,
            title: title.into(),
            install_path: install_path.into(),
            launcher_id,
            language: None,
            leftover: false,
            patch_state: None,
            installed: None,
            playable: None,
            last_played_unix: None,
        }
    }
}

/// Discovery knobs. GDK / XboxGames is **opt-in**; default is off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverOptions {
    /// When false (default), XboxGames / GDK titles are not probed at all.
    pub include_xbox_games: bool,
}

impl Default for DiscoverOptions {
    fn default() -> Self {
        Self {
            include_xbox_games: false,
        }
    }
}

/// Combined discovery result. Missing launchers yield empty store slices,
/// never a hard failure.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscoverReport {
    pub titles: Vec<DiscoveredTitle>,
    pub warnings: Vec<crate::error::StoreWarning>,
}

impl DiscoverReport {
    pub fn titles_for(&self, store: StoreId) -> impl Iterator<Item = &DiscoveredTitle> {
        self.titles.iter().filter(move |t| t.store == store)
    }
}
