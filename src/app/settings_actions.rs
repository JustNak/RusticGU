//! Settings disk helpers and live-draft setters.

use std::path::PathBuf;

use gpui::{Context, ParentElement, Window};
use gpui_component::WindowExt;

use super::LibraryApp;
use crate::appearance::{apply_appearance, apply_window_opacity};
use crate::persistence::save_settings;
use crate::settings::{
    custom_directory_key, custom_directory_reject_reason, normalize_custom_game_directory,
    AccentPreset, AppTheme, CornerRadiusScale, OsNotifyMode, ProgressStyle, Settings, UiDensity,
    UpdateChannel,
};
use crate::startup::apply_launch_at_startup;

impl LibraryApp {
    pub(crate) fn save_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.sanitize_appearance();
        if let Err(msg) = save_settings(&self.paths, &self.settings) {
            self.show_error_toast(msg, cx);
            return;
        }
        apply_appearance(&self.settings, Some(window), cx);
        self.live
            .set_allow_dstorage(self.settings.allow_dstorage_override);
        self.sync_tray_lifetime(cx);
        let _ = apply_launch_at_startup(
            self.settings.launch_at_startup,
            self.settings.startup_minimized,
        );
        self.show_toast("Settings saved.", cx);
        cx.notify();
    }

    pub(crate) fn preview_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        apply_appearance(&self.settings, Some(window), cx);
        self.applied_window_transparency = Some(self.settings.window_transparency);
        cx.notify();
    }

    pub(crate) fn set_theme_draft(
        &mut self,
        theme: AppTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.theme = theme;
        self.preview_appearance(window, cx);
    }

    pub(crate) fn set_accent_preset(
        &mut self,
        preset: AccentPreset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.accent_preset = preset;
        self.preview_appearance(window, cx);
    }

    pub(crate) fn set_backdrop_blur(
        &mut self,
        on: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.backdrop_blur = on;
        apply_window_opacity(
            window,
            self.settings.window_transparency,
            self.settings.backdrop_blur,
        );
        cx.notify();
    }

    pub(crate) fn set_ui_density(
        &mut self,
        density: UiDensity,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.ui_density = density;
        self.preview_appearance(window, cx);
    }

    pub(crate) fn set_corner_radius(
        &mut self,
        scale: CornerRadiusScale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.corner_radius = scale;
        self.preview_appearance(window, cx);
    }

    pub(crate) fn set_reduce_motion(
        &mut self,
        on: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.reduce_motion = on;
        cx.notify();
    }

    pub(crate) fn set_progress_style(
        &mut self,
        style: ProgressStyle,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.progress_style = style;
        cx.notify();
    }

    pub(crate) fn reset_appearance_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.reset_appearance();
        self.sync_appearance_sliders(window, cx);
        self.preview_appearance(window, cx);
    }

    pub(crate) fn set_close_to_tray(
        &mut self,
        on: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.close_to_tray = on;
        self.sync_tray_lifetime(cx);
        cx.notify();
    }

    pub(crate) fn set_launch_at_startup(
        &mut self,
        on: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.launch_at_startup = on;
        if !on {
            self.settings.startup_minimized = false;
        }
        let _ = apply_launch_at_startup(
            self.settings.launch_at_startup,
            self.settings.startup_minimized,
        );
        cx.notify();
    }

    pub(crate) fn set_startup_minimized(
        &mut self,
        on: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.startup_minimized = on;
        if self.settings.launch_at_startup {
            let _ = apply_launch_at_startup(true, on);
        }
        cx.notify();
    }

    pub(crate) fn set_os_notify_mode(
        &mut self,
        mode: OsNotifyMode,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.os_notify_mode = mode;
        self.sync_tray_lifetime(cx);
        cx.notify();
    }

    pub(crate) fn set_notify_on_complete(
        &mut self,
        on: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.notify_on_complete = on;
        cx.notify();
    }

    pub(crate) fn set_notify_on_fail(
        &mut self,
        on: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.notify_on_fail = on;
        cx.notify();
    }

    pub(crate) fn set_include_xbox_games(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.settings.include_xbox_games == on {
            return;
        }
        self.settings.include_xbox_games = on;
        self.refresh_library(cx);
        cx.notify();
    }

    pub(crate) fn prompt_add_custom_game_directory(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        super::widgets::prompt_custom_game_directory(cx.entity(), window, cx);
    }

    pub(crate) fn add_custom_game_directory(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = normalize_custom_game_directory(&path) else {
            self.show_error_toast("Choose a game folder.", cx);
            return;
        };
        if let Some(reason) = custom_directory_reject_reason(&path) {
            self.show_error_toast(reason, cx);
            return;
        }
        if !path.is_dir() {
            self.show_error_toast("That folder does not exist.", cx);
            return;
        }
        let key = custom_directory_key(&path);
        if self
            .settings
            .custom_game_directories
            .iter()
            .any(|p| custom_directory_key(p) == key)
        {
            self.show_toast("That folder is already in Custom games.", cx);
            return;
        }
        if self
            .games
            .iter()
            .any(|g| custom_directory_key(&g.install_path) == key)
        {
            self.show_toast("That folder is already in your library.", cx);
            return;
        }
        self.settings.custom_game_directories.push(path);
        if !self.persist_settings_quietly(window, cx) {
            return;
        }
        self.show_toast("Custom game folder added.", cx);
        self.refresh_library(cx);
        cx.notify();
    }

    pub(crate) fn remove_custom_game_directory(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = custom_directory_key(&path);
        let before = self.settings.custom_game_directories.len();
        self.settings
            .custom_game_directories
            .retain(|p| custom_directory_key(p) != key);
        if self.settings.custom_game_directories.len() == before {
            return;
        }
        if !self.persist_settings_quietly(window, cx) {
            return;
        }
        self.show_toast("Custom game folder removed.", cx);
        self.refresh_library(cx);
        cx.notify();
    }

    fn persist_settings_quietly(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.settings.sanitize_appearance();
        if let Err(msg) = save_settings(&self.paths, &self.settings) {
            self.show_error_toast(msg, cx);
            return false;
        }
        apply_appearance(&self.settings, Some(window), cx);
        true
    }

    pub(crate) fn set_update_channel(
        &mut self,
        channel: UpdateChannel,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings.update_channel == channel {
            return;
        }
        self.settings.update_channel = channel;
        self.available_update = None;
        self.update_check_gen = self.update_check_gen.wrapping_add(1);
        self.update_busy = false;
        cx.notify();
    }

    pub(crate) fn confirm_reset_settings_defaults(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_reset_defaults_dialog(window, cx);
    }

    fn open_reset_defaults_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        window.open_dialog(cx, move |dialog, _window, _cx| {
            let view = view.clone();
            dialog
                .title("Reset settings?")
                .confirm()
                .child("Restore factory defaults? Window size and position are kept.")
                .on_ok(move |_, window, cx| {
                    view.update(cx, |app, cx| {
                        app.settings.reset_to_defaults_preserving_layout();
                        app.sync_appearance_sliders(window, cx);
                        app.preview_appearance(window, cx);
                        app.sync_tray_lifetime(cx);
                        app.refresh_library(cx);
                        app.show_toast("Defaults restored (Save to persist).", cx);
                    });
                    true
                })
        });
    }

    #[allow(dead_code)]
    fn _keep_settings_type(_s: Settings) {}
}
