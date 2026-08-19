use std::path::PathBuf;

use crate::acf::{parse_vdf_path, VdfObject};
use crate::error::{WatchError, WatchResult};
use crate::flags::PatchingSignals;

/// One Steam title as observed by ACF + the downloading probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleStatus {
    pub app_id: u32,
    pub name: String,
    pub install_dir: PathBuf,
    pub state_flags: u32,
    pub bytes_to_download: u64,
    pub bytes_downloaded: u64,
    pub steam_downloading: bool,
}

impl TitleStatus {
    pub fn title_id(&self) -> String {
        self.app_id.to_string()
    }

    pub fn signals(&self) -> PatchingSignals {
        PatchingSignals {
            state_flags: self.state_flags,
            bytes_to_download: self.bytes_to_download,
            bytes_downloaded: self.bytes_downloaded,
            steam_downloading: self.steam_downloading,
        }
    }

    pub fn is_patching(&self) -> bool {
        self.signals().is_patching()
    }
}

/// Injected Steam downloading + ACF source. Tests feed state transitions.
pub trait SteamStatus {
    fn snapshot(&self) -> WatchResult<Vec<TitleStatus>>;
}

/// Scriptable in-memory source. Tests mutate `titles` between ticks.
#[derive(Debug, Default, Clone)]
pub struct MemorySteam {
    pub titles: Vec<TitleStatus>,
}

impl SteamStatus for MemorySteam {
    fn snapshot(&self) -> WatchResult<Vec<TitleStatus>> {
        Ok(self.titles.clone())
    }
}

impl MemorySteam {
    pub fn new(titles: Vec<TitleStatus>) -> Self {
        Self { titles }
    }

    pub fn set_flags(&mut self, app_id: u32, flags: u32) {
        if let Some(t) = self.titles.iter_mut().find(|t| t.app_id == app_id) {
            t.state_flags = flags;
        }
    }

    pub fn set_bytes(&mut self, app_id: u32, to_download: u64, downloaded: u64) {
        if let Some(t) = self.titles.iter_mut().find(|t| t.app_id == app_id) {
            t.bytes_to_download = to_download;
            t.bytes_downloaded = downloaded;
        }
    }

    pub fn set_downloading_probe(&mut self, app_id: u32, downloading: bool) {
        if let Some(t) = self.titles.iter_mut().find(|t| t.app_id == app_id) {
            t.steam_downloading = downloading;
        }
    }
}

/// Build a [`TitleStatus`] from parsed ACF `AppState` plus an optional probe.
pub fn title_from_acf(app: &VdfObject, steam_downloading: bool) -> WatchResult<TitleStatus> {
    let app_id = app
        .get_u32("appid")
        .or_else(|| app.get_u32("AppID"))
        .ok_or_else(|| WatchError::Status("ACF missing appid".into()))?;
    let name = app.get("name").unwrap_or("").to_string();
    let install_dir = app.get("installdir").unwrap_or("").to_string();
    Ok(TitleStatus {
        app_id,
        name,
        install_dir: PathBuf::from(install_dir),
        state_flags: app.get_u32("StateFlags").unwrap_or(0),
        bytes_to_download: app.get_u64("BytesToDownload").unwrap_or(0),
        bytes_downloaded: app.get_u64("BytesDownloaded").unwrap_or(0),
        steam_downloading,
    })
}

pub fn title_from_acf_text(
    path: &std::path::Path,
    text: &str,
    steam_downloading: bool,
) -> WatchResult<TitleStatus> {
    let v = parse_vdf_path(path, text)?;
    title_from_acf(v.app_state(), steam_downloading)
}

impl<T: SteamStatus + ?Sized> SteamStatus for &T {
    fn snapshot(&self) -> WatchResult<Vec<TitleStatus>> {
        (**self).snapshot()
    }
}

impl<T: SteamStatus + ?Sized> SteamStatus for &mut T {
    fn snapshot(&self) -> WatchResult<Vec<TitleStatus>> {
        (**self).snapshot()
    }
}
