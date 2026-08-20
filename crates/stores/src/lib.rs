//! Extra-launcher game discovery from **official indexes only**.
//!
//! Sources (all optional / skip-if-absent), in order:
//! 1. Epic Manifests (`ProgramData/Epic/EpicGamesLauncher/Data/Manifests`)
//! 2. GOG HKLM `SOFTWARE\WOW6432Node\GOG.com\Games` (and non-WOW)
//! 3. EA Desktop / Origin local content + JSON catalog
//! 4. Ubisoft Connect registry `Installs\{id}` (InstallDir + Language);
//!    never `configurations/ownership`
//! 5. Riot Client `Metadata` (`product_install_full_path` or leftover
//!    `product_install_root`; `update-status.json` is patch state)
//! 6. Battle.net Agent `product_installs[]` (skip `agent` / `bna`)
//! 7. itch butlerd `Fetch.Caves` only (never `butler.db`)
//! 8. GDK / XboxGames: **opt-in only**
//!
//! Hard safety:
//! - never volume-walk (`D:\`)
//! - never open itch `butler.db`
//! - never touch `WindowsApps` / takeown
//! - no cross-store dedupe (Steam+EGS style dual-register is allowed)
//!
//! Windows registry and disk access are traits so Linux unit tests inject
//! fixtures and in-memory fakes.

pub mod battlenet;
pub mod ea;
pub mod epic;
pub mod error;
pub mod fs;
pub mod gog;
pub mod itch;
pub mod model;
pub mod probe;
pub mod registry;
pub mod riot;
pub mod roots;
pub mod ubisoft;
pub mod util;
pub mod xbox;

pub use error::{StoreError, StoreResult, StoreWarning};
pub use fs::{IndexFs, MemoryFs, RecordingFs, StdFs};
pub use model::{DiscoverOptions, DiscoverReport, DiscoveredTitle, StoreId};
pub use probe::{discover_all, StoreProbe};
pub use registry::{EmptyHive, MemoryHive, RegistryHive};
pub use roots::PathRoots;
