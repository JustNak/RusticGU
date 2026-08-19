use crate::error::StoreWarning;
use crate::fs::IndexFs;
use crate::model::{DiscoverOptions, DiscoverReport, StoreId};
use crate::registry::RegistryHive;
use crate::roots::PathRoots;
use crate::{battlenet, ea, epic, gog, itch, riot, ubisoft, xbox};

/// Discovers titles from official launcher indexes in a fixed order.
///
/// 1. Epic Manifests  
/// 2. GOG HKLM Games  
/// 3. EA, Ubisoft, Riot, Battle.net, itch (each via its own index)  
/// 4. XboxGames / GDK — only when `opts.include_xbox_games` is true
///
/// Missing launchers produce an empty contribution, not a hard error.
/// Dual-registered titles may appear twice (no cross-store dedupe).
pub struct StoreProbe<F, R> {
    pub fs: F,
    pub registry: R,
    pub roots: PathRoots,
}

impl<F: IndexFs, R: RegistryHive> StoreProbe<F, R> {
    pub fn new(fs: F, registry: R, roots: PathRoots) -> Self {
        Self { fs, registry, roots }
    }

    pub fn discover_all(&self, opts: &DiscoverOptions) -> DiscoverReport {
        let mut report = DiscoverReport::default();

        if let Some(dir) = &self.roots.epic_manifests {
            let (t, w) = epic::discover(&self.fs, dir);
            report.titles.extend(t);
            report.warnings.extend(w);
        }

        {
            let (t, w) = gog::discover(&self.registry);
            report.titles.extend(t);
            report.warnings.extend(w);
        }

        {
            let (t, w) = ea::discover(
                &self.fs,
                self.roots.origin_local_content.as_deref(),
                self.roots.ea_install_index.as_deref(),
            );
            report.titles.extend(t);
            report.warnings.extend(w);
        }

        {
            let (t, w) = ubisoft::discover(
                &self.registry,
                &self.fs,
                self.roots.ubisoft_index.as_deref(),
            );
            report.titles.extend(t);
            report.warnings.extend(w);
        }

        {
            let (t, w) = riot::discover(
                &self.fs,
                self.roots.riot_metadata.as_deref(),
                self.roots.riot_installs.as_deref(),
            );
            report.titles.extend(t);
            report.warnings.extend(w);
        }

        if let Some(dir) = &self.roots.battlenet_agent {
            let (t, w) = battlenet::discover(&self.fs, dir);
            report.titles.extend(t);
            report.warnings.extend(w);
        }

        if let Some(dir) = &self.roots.itch_config {
            let (t, w) = itch::discover(&self.fs, dir);
            report.titles.extend(t);
            report.warnings.extend(w);
        }

        if opts.include_xbox_games {
            let (t, w) = xbox::discover(&self.fs, &self.roots.xbox_games_roots);
            report.titles.extend(t);
            report.warnings.extend(w);
        } else if !self.roots.xbox_games_roots.is_empty() {
            report.warnings.push(StoreWarning::new(
                StoreId::XboxGames,
                "XboxGames/GDK skipped (opt-in flag is off)",
            ));
        }

        report
    }
}

/// Convenience: discover with injected backends (the usual test / Linux path).
pub fn discover_all<F: IndexFs, R: RegistryHive>(
    fs: &F,
    registry: &R,
    roots: &PathRoots,
    opts: &DiscoverOptions,
) -> DiscoverReport {
    StoreProbe {
        fs,
        registry,
        roots: roots.clone(),
    }
    .discover_all(opts)
}
