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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredTitle {
    pub store: StoreId,
    pub title: String,
    pub install_path: PathBuf,
    pub launcher_id: Option<String>,
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
