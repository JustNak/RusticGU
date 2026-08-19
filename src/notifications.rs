//! OS tray balloon helpers for compact completion / failure.

use crate::settings::OsNotifyMode;
use crate::tray::{NotifyLevel, SystemTray};

/// Whether an OS balloon should fire for this completion.
pub fn should_notify_os(mode: OsNotifyMode, window_hidden_to_tray: bool) -> bool {
    match mode {
        OsNotifyMode::Off => false,
        OsNotifyMode::Always => true,
        OsNotifyMode::WhenHiddenToTray => window_hidden_to_tray,
    }
}

/// Best-effort tray balloon. No-op when the tray is missing or policy says skip.
pub fn notify_compact(
    tray: Option<&SystemTray>,
    mode: OsNotifyMode,
    window_hidden_to_tray: bool,
    title: &str,
    body: &str,
    ok: bool,
) {
    if !should_notify_os(mode, window_hidden_to_tray) {
        return;
    }
    let Some(tray) = tray else {
        return;
    };
    let level = if ok {
        NotifyLevel::Info
    } else {
        NotifyLevel::Error
    };
    tray.show_notification(title, body, level, 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_mode_only_fires_when_hidden() {
        assert!(!should_notify_os(OsNotifyMode::Off, true));
        assert!(!should_notify_os(OsNotifyMode::WhenHiddenToTray, false));
        assert!(should_notify_os(OsNotifyMode::WhenHiddenToTray, true));
        assert!(should_notify_os(OsNotifyMode::Always, false));
    }
}
