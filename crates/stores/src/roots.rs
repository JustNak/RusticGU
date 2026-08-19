use std::env;
use std::path::PathBuf;

/// Official index locations. Every field is optional: missing path = skip.
/// Never point these at a volume root.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathRoots {
    /// `ProgramData/Epic/EpicGamesLauncher/Data/Manifests`
    pub epic_manifests: Option<PathBuf>,
    /// `ProgramData/Origin/LocalContent`
    pub origin_local_content: Option<PathBuf>,
    /// EA Desktop / EA App JSON install catalog (decrypted/export or sidecar).
    pub ea_install_index: Option<PathBuf>,
    /// Optional Ubisoft Connect JSON game list (`games.json`).
    pub ubisoft_index: Option<PathBuf>,
    /// `ProgramData/Riot Games/Metadata`
    pub riot_metadata: Option<PathBuf>,
    /// `ProgramData/Riot Games/RiotClientInstalls.json`
    pub riot_installs: Option<PathBuf>,
    /// `ProgramData/Battle.net/Agent` (JSON product records only).
    pub battlenet_agent: Option<PathBuf>,
    /// itch config dir (`%APPDATA%/itch`) — never `butler.db`.
    pub itch_config: Option<PathBuf>,
    /// Explicit XboxGames library folders. Used only when opt-in.
    pub xbox_games_roots: Vec<PathBuf>,
}

impl PathRoots {
    /// Typical Windows locations derived from environment variables.
    /// On Linux those vars are usually unset, so every source stays `None`.
    pub fn from_env() -> Self {
        let program_data = env::var_os("PROGRAMDATA")
            .or_else(|| env::var_os("ProgramData"))
            .map(PathBuf::from);
        let appdata = env::var_os("APPDATA").map(PathBuf::from);
        let mut roots = Self::default();
        if let Some(pd) = &program_data {
            roots.epic_manifests = Some(
                pd.join("Epic")
                    .join("EpicGamesLauncher")
                    .join("Data")
                    .join("Manifests"),
            );
            roots.origin_local_content = Some(pd.join("Origin").join("LocalContent"));
            roots.ea_install_index = Some(
                pd.join("Electronic Arts")
                    .join("EA Desktop")
                    .join("install_index.json"),
            );
            roots.riot_metadata = Some(pd.join("Riot Games").join("Metadata"));
            roots.riot_installs = Some(pd.join("Riot Games").join("RiotClientInstalls.json"));
            roots.battlenet_agent = Some(pd.join("Battle.net").join("Agent"));
        }
        if let Some(ad) = &appdata {
            roots.itch_config = Some(ad.join("itch"));
        }
        roots
    }
}
