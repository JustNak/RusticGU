use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::branding::APP_VERSION;
use crate::settings::Settings;
use crate::updater::normalize_version;

fn write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root: PathBuf,
    pub settings: PathBuf,
    pub state: PathBuf,
    /// Snapshot written before update handoff; shown once after relaunch.
    pub pending_whats_new: PathBuf,
}

pub fn app_paths() -> AppPaths {
    let root = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(crate::branding::APP_DATA_DIR_NAME);
    AppPaths {
        settings: root.join("settings.json"),
        state: root.join("state.json"),
        pending_whats_new: root.join("pending_whats_new.json"),
        root,
    }
}

/// Release notes snapshot for the post-update “What’s new” dialog.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingWhatsNew {
    pub from_version: String,
    pub to_version: String,
    #[serde(default)]
    pub release_name: String,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub notes: Option<String>,
}

impl PendingWhatsNew {
    pub fn matches_running_app(&self) -> bool {
        normalize_version(&self.to_version) == normalize_version(APP_VERSION)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_app_id: Option<u32>,
    #[serde(default)]
    pub last_compact_app_id: Option<u32>,
}

pub fn load_pending_whats_new(paths: &AppPaths) -> Option<PendingWhatsNew> {
    let bytes = fs::read(&paths.pending_whats_new).ok()?;
    let pending: PendingWhatsNew = match serde_json::from_slice(&bytes) {
        Ok(pending) => pending,
        Err(_) => {
            let _ = clear_pending_whats_new(paths);
            return None;
        }
    };
    if pending.matches_running_app() {
        Some(pending)
    } else {
        let _ = clear_pending_whats_new(paths);
        None
    }
}

pub fn save_pending_whats_new(paths: &AppPaths, pending: &PendingWhatsNew) -> Result<(), String> {
    let _guard = write_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    ensure_app_dirs(paths)?;
    let json = serde_json::to_vec_pretty(pending)
        .map_err(|e| format!("Could not serialize What’s new snapshot: {e}"))?;
    atomic_write(&paths.pending_whats_new, &json)
}

pub fn clear_pending_whats_new(paths: &AppPaths) -> Result<(), String> {
    let _guard = write_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match fs::remove_file(&paths.pending_whats_new) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Could not clear What’s new snapshot: {e}")),
    }
}

pub fn ensure_app_dirs(paths: &AppPaths) -> Result<(), String> {
    fs::create_dir_all(&paths.root).map_err(|e| format!("Could not create app data dir: {e}"))
}

pub fn load_settings(paths: &AppPaths) -> Settings {
    let Ok(bytes) = fs::read(&paths.settings) else {
        return Settings::default();
    };
    let mut settings: Settings = serde_json::from_slice(&bytes).unwrap_or_default();
    settings.sanitize_appearance();
    settings
}

pub fn save_settings(paths: &AppPaths, settings: &Settings) -> Result<(), String> {
    let _guard = write_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    ensure_app_dirs(paths)?;
    let json = serde_json::to_vec_pretty(settings)
        .map_err(|e| format!("Could not serialize settings: {e}"))?;
    atomic_write(&paths.settings, &json)
}

pub fn load_state(paths: &AppPaths) -> AppState {
    let Ok(bytes) = fs::read(&paths.state) else {
        return AppState::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub fn save_state(paths: &AppPaths, state: &AppState) -> Result<(), String> {
    let _guard = write_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    ensure_app_dirs(paths)?;
    let json = serde_json::to_vec(state).map_err(|e| format!("Could not serialize state: {e}"))?;
    atomic_write(&paths.state, &json)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Persistence path has no parent directory.".to_string())?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("data.json");
    let temp_path = parent.join(format!(".{file_name}.tmp"));
    fs::write(&temp_path, bytes).map_err(|e| format!("Could not write temp file: {e}"))?;
    fs::rename(&temp_path, path).map_err(|e| {
        let _ = fs::remove_file(&temp_path);
        format!("Could not finalize write: {e}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_paths(tag: &str) -> AppPaths {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!("rusticgu-persist-{tag}-{nanos}"));
        let _ = fs::create_dir_all(&root);
        AppPaths {
            settings: root.join("settings.json"),
            state: root.join("state.json"),
            pending_whats_new: root.join("pending_whats_new.json"),
            root,
        }
    }

    #[test]
    fn settings_and_state_use_camel_case() {
        let paths = temp_paths("camel");
        let mut settings = Settings::default();
        settings.close_to_tray = false;
        save_settings(&paths, &settings).unwrap();
        let text = fs::read_to_string(&paths.settings).unwrap();
        assert!(text.contains("\"closeToTray\""));
        assert!(!text.contains("\"close_to_tray\""));

        let state = AppState {
            selected_app_id: Some(730),
            last_compact_app_id: Some(570),
        };
        save_state(&paths, &state).unwrap();
        let state_text = fs::read_to_string(&paths.state).unwrap();
        assert!(state_text.contains("\"selectedAppId\""));
        assert!(state_text.contains("\"lastCompactAppId\""));
        assert_eq!(load_state(&paths), state);
        let _ = fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn pending_whats_new_round_trip() {
        let paths = temp_paths("round");
        let pending = PendingWhatsNew {
            from_version: "0.1.0".into(),
            to_version: APP_VERSION.into(),
            release_name: "Test".into(),
            html_url: "https://example.com".into(),
            notes: Some("- fix one\n- fix two".into()),
        };
        save_pending_whats_new(&paths, &pending).unwrap();
        let loaded = load_pending_whats_new(&paths).expect("matching pending");
        assert_eq!(loaded, pending);
        clear_pending_whats_new(&paths).unwrap();
        assert!(load_pending_whats_new(&paths).is_none());
        let _ = fs::remove_dir_all(&paths.root);
    }
}
