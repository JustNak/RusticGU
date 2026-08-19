mod about_dialog;
mod confirm_dialogs;
mod filter;
mod library_view;
mod settings_actions;
mod settings_category;
mod settings_panel;
mod sidebar;
mod title_bar;
mod toast;
mod tray_flyout;
mod tray_lifecycle;
mod update_flow;
mod widgets;

pub use filter::FilterKind;
pub(crate) use settings_category::SettingsCategory;

use std::time::Instant;

use gpui::{
    canvas, div, prelude::FluentBuilder, App, AppContext, Context, Focusable, InteractiveElement,
    IntoElement, ParentElement, Render, Styled, Window, WindowBounds,
};
use gpui_component::{
    input::InputState,
    slider::{SliderEvent, SliderState},
    v_flex, ActiveTheme, Root,
};

use crate::activate::ActivateBridge;
use crate::appearance::{
    apply_appearance, film_grain_image, noise_enabled, vignette_edge_alpha, vignette_enabled,
};
use crate::compact::{
    apply_compact, estimate_compact, preflight, CompactOp, CompactProgress, CompactRefuse,
};
use crate::library::{scan_steam_library, SteamGame};
use crate::notifications::notify_compact;
use crate::persistence::{
    load_pending_whats_new, load_state, save_settings, save_state, AppPaths, AppState,
    PendingWhatsNew,
};
use crate::settings::{Settings, WindowLayout};
use crate::startup::launched_minimized;
use crate::tray::SystemTray;
use crate::updater::UpdateInfo;
use toast::Toast;
use widgets::render_vignette_overlay;

pub struct LibraryApp {
    pub(crate) games: Vec<SteamGame>,
    pub(crate) settings: Settings,
    pub(crate) paths: AppPaths,
    pub(crate) filter: FilterKind,
    pub(crate) settings_return_filter: FilterKind,
    pub(crate) settings_category: SettingsCategory,
    pub(crate) selected_app_id: Option<u32>,
    pub(crate) library_scanning: bool,
    pub(crate) library_error: Option<String>,
    pub(crate) compact_busy: bool,
    pub(crate) compact_progress: Option<CompactProgress>,
    pub(crate) toasts: Vec<Toast>,
    pub(crate) next_toast_id: u64,
    pub(crate) pending_toast: Option<String>,
    pub(crate) search_input: gpui::Entity<InputState>,
    pub(crate) noise_slider: gpui::Entity<SliderState>,
    pub(crate) opacity_slider: gpui::Entity<SliderState>,
    pub(crate) hue_slider: gpui::Entity<SliderState>,
    pub(crate) sat_slider: gpui::Entity<SliderState>,
    pub(crate) light_slider: gpui::Entity<SliderState>,
    pub(crate) vignette_slider: gpui::Entity<SliderState>,
    pub(crate) applied_window_transparency: Option<u8>,
    pub(crate) window_layout_dirty: bool,
    pub(crate) last_window_layout_save: Instant,
    pub(crate) update_busy: bool,
    pub(crate) update_check_gen: u64,
    pub(crate) available_update: Option<UpdateInfo>,
    pub(crate) update_toast_id: Option<u64>,
    pub(crate) pending_whats_new: Option<PendingWhatsNew>,
    pub(crate) pending_show_whats_new: bool,
    pub(crate) system_tray: Option<SystemTray>,
    pub(crate) force_quit: bool,
    pub(crate) window_hidden_to_tray: bool,
    pub(crate) main_hwnd: isize,
    pub(crate) pending_tray_show: bool,
    pub(crate) pending_toggle_flyout: bool,
    pub(crate) flyout_open: bool,
    pub(crate) activate: ActivateBridge,
}

impl LibraryApp {
    pub fn new(
        settings: Settings,
        paths: AppPaths,
        activate: ActivateBridge,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search games…")
                .clean_on_escape()
        });
        let noise_slider = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(100.)
                .step(1.)
                .default_value(settings.noise_intensity as f32)
        });
        let opacity_slider = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(100.)
                .step(1.)
                .default_value(settings.window_transparency as f32)
        });
        let hue_slider = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(360.)
                .step(1.)
                .default_value(settings.accent_hue)
        });
        let sat_slider = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(100.)
                .step(1.)
                .default_value(settings.accent_saturation)
        });
        let light_slider = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(100.)
                .step(1.)
                .default_value(settings.accent_lightness)
        });
        let vignette_slider = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(100.)
                .step(1.)
                .default_value(settings.vignette_intensity as f32)
        });

        let state = load_state(&paths);
        let pending_whats_new = load_pending_whats_new(&paths);
        let pending_show_whats_new = pending_whats_new.is_some();

        apply_appearance(&settings, Some(window), cx);

        let mut app = Self {
            games: Vec::new(),
            settings,
            paths,
            filter: FilterKind::Library,
            settings_return_filter: FilterKind::Library,
            settings_category: SettingsCategory::General,
            selected_app_id: state.selected_app_id,
            library_scanning: true,
            library_error: None,
            compact_busy: false,
            compact_progress: None,
            toasts: Vec::new(),
            next_toast_id: 1,
            pending_toast: None,
            search_input,
            noise_slider,
            opacity_slider,
            hue_slider,
            sat_slider,
            light_slider,
            vignette_slider,
            applied_window_transparency: None,
            window_layout_dirty: false,
            last_window_layout_save: Instant::now(),
            update_busy: false,
            update_check_gen: 0,
            available_update: None,
            update_toast_id: None,
            pending_whats_new,
            pending_show_whats_new,
            system_tray: None,
            force_quit: false,
            window_hidden_to_tray: launched_minimized(),
            main_hwnd: 0,
            pending_tray_show: false,
            pending_toggle_flyout: false,
            flyout_open: false,
            activate,
        };
        app.subscribe_sliders(cx);
        app.sync_tray_lifetime(cx);
        app.refresh_library(cx);

        let entity = cx.entity();
        window.on_window_should_close(cx, move |window, cx| {
            entity.update(cx, |app, cx| app.handle_window_should_close(window, cx))
        });

        app
    }

    fn subscribe_sliders(&mut self, cx: &mut Context<Self>) {
        cx.subscribe(&self.noise_slider, |this, _, ev: &SliderEvent, cx| {
            let SliderEvent::Change(v) = ev;
            this.settings.noise_intensity = v.start().round().clamp(0.0, 100.0) as u8;
            cx.notify();
        })
        .detach();
        cx.subscribe(&self.opacity_slider, |this, _, ev: &SliderEvent, cx| {
            let SliderEvent::Change(v) = ev;
            this.settings.window_transparency = v.start().round().clamp(0.0, 100.0) as u8;
            cx.notify();
        })
        .detach();
        cx.subscribe(&self.hue_slider, |this, _, ev: &SliderEvent, cx| {
            let SliderEvent::Change(v) = ev;
            this.settings.accent_hue = v.start().rem_euclid(360.0);
            cx.notify();
        })
        .detach();
        cx.subscribe(&self.sat_slider, |this, _, ev: &SliderEvent, cx| {
            let SliderEvent::Change(v) = ev;
            this.settings.accent_saturation = v.start().clamp(0.0, 100.0);
            cx.notify();
        })
        .detach();
        cx.subscribe(&self.light_slider, |this, _, ev: &SliderEvent, cx| {
            let SliderEvent::Change(v) = ev;
            this.settings.accent_lightness = v.start().clamp(0.0, 100.0);
            cx.notify();
        })
        .detach();
        cx.subscribe(&self.vignette_slider, |this, _, ev: &SliderEvent, cx| {
            let SliderEvent::Change(v) = ev;
            this.settings.vignette_intensity = v.start().round().clamp(0.0, 100.0) as u8;
            cx.notify();
        })
        .detach();
    }

    pub(crate) fn sync_appearance_sliders(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let s = &self.settings;
        self.noise_slider.update(cx, |slider, cx| {
            slider.set_value(s.noise_intensity as f32, window, cx);
        });
        self.opacity_slider.update(cx, |slider, cx| {
            slider.set_value(s.window_transparency as f32, window, cx);
        });
        self.hue_slider.update(cx, |slider, cx| {
            slider.set_value(s.accent_hue, window, cx);
        });
        self.sat_slider.update(cx, |slider, cx| {
            slider.set_value(s.accent_saturation, window, cx);
        });
        self.light_slider.update(cx, |slider, cx| {
            slider.set_value(s.accent_lightness, window, cx);
        });
        self.vignette_slider.update(cx, |slider, cx| {
            slider.set_value(s.vignette_intensity as f32, window, cx);
        });
    }

    pub(crate) fn refresh_library(&mut self, cx: &mut Context<Self>) {
        self.library_scanning = true;
        self.library_error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async { scan_steam_library() })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.library_scanning = false;
                match result {
                    Ok(games) => {
                        app.games = games;
                        app.library_error = None;
                        if app.selected_app_id.is_none() {
                            app.selected_app_id = app.games.first().map(|g| g.app_id);
                        }
                    }
                    Err(msg) => {
                        app.games.clear();
                        app.library_error = Some(msg);
                    }
                }
                app.flush_state_now();
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn select_filter(
        &mut self,
        filter: FilterKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if filter == FilterKind::Settings {
            if self.filter != FilterKind::Settings {
                self.settings_return_filter = self.filter;
            }
            self.filter = FilterKind::Settings;
        } else {
            self.filter = filter;
        }
        let _ = window;
        cx.notify();
    }

    pub(crate) fn leave_settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.filter = if self.settings_return_filter == FilterKind::Settings {
            FilterKind::Library
        } else {
            self.settings_return_filter
        };
        cx.notify();
    }

    pub(crate) fn select_game(&mut self, app_id: u32, cx: &mut Context<Self>) {
        self.selected_app_id = Some(app_id);
        self.flush_state_now();
        cx.notify();
    }

    pub(crate) fn library_counts(&self) -> (i32, i32, i32) {
        let all = self.games.len() as i32;
        let compacted = self.games.iter().filter(|g| is_compacted(g)).count() as i32;
        (all, compacted, all - compacted)
    }

    pub(crate) fn visible_games(&self, cx: &App) -> Vec<SteamGame> {
        let query = self
            .search_input
            .read(cx)
            .value()
            .trim()
            .to_ascii_lowercase();
        self.games
            .iter()
            .filter(|game| match self.filter {
                FilterKind::Library | FilterKind::Settings => true,
                FilterKind::Compacted => is_compacted(game),
                FilterKind::Uncompacted => !is_compacted(game),
            })
            .filter(|game| {
                query.is_empty()
                    || game.name.to_ascii_lowercase().contains(&query)
                    || game.app_id.to_string().contains(&query)
            })
            .cloned()
            .collect()
    }

    pub(crate) fn selected_game(&self) -> Option<&SteamGame> {
        let id = self.selected_app_id?;
        self.games.iter().find(|g| g.app_id == id)
    }

    pub(crate) fn estimate_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(game) = self.selected_game().cloned() else {
            self.show_toast("Select a game first.", cx);
            return;
        };
        let algorithm = self.settings.compact_algorithm.for_live_library();
        match estimate_compact(&game.install_path, algorithm) {
            Ok(estimate) => {
                self.open_estimate_dialog(window, cx, estimate, game.name);
            }
            Err(msg) => self.show_error_toast(msg, cx),
        }
    }

    pub(crate) fn start_compact(
        &mut self,
        op: CompactOp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.compact_busy {
            self.show_toast("A compact job is already running.", cx);
            return;
        }
        let Some(game) = self.selected_game().cloned() else {
            self.show_toast("Select a game first.", cx);
            return;
        };
        match preflight(&game.install_path, self.settings.allow_dstorage_override) {
            Err(CompactRefuse::DirectStorage { .. }) => {
                if let Ok(estimate) = estimate_compact(
                    &game.install_path,
                    self.settings.compact_algorithm.for_live_library(),
                ) {
                    self.open_dstorage_warning(window, cx, estimate);
                } else {
                    self.show_error_toast(
                        "dstorage.dll is present. Enable the override in Settings → General to compact.",
                        cx,
                    );
                }
                return;
            }
            Err(err) => {
                self.show_error_toast(err.to_string(), cx);
                return;
            }
            Ok(()) => {}
        }

        let algorithm = self.settings.compact_algorithm.for_live_library();
        match estimate_compact(&game.install_path, algorithm) {
            Ok(estimate) => {
                let verb = match op {
                    CompactOp::Compress => "Compact",
                    CompactOp::Uncompress => "Uncompact",
                };
                self.show_toast(
                    format!(
                        "{verb} estimate: {} files, skip {}.",
                        estimate.file_count, estimate.skipped_count
                    ),
                    cx,
                );
            }
            Err(msg) => {
                self.show_error_toast(msg, cx);
                return;
            }
        }

        self.compact_busy = true;
        self.compact_progress = Some(CompactProgress {
            processed: 0,
            total: 1,
            message: "Starting…".into(),
        });
        cx.notify();

        let allow = self.settings.allow_dstorage_override;
        let path = game.install_path.clone();
        let name = game.name.clone();
        let (tx, rx) = async_channel::unbounded::<Result<CompactProgress, String>>();
        std::thread::spawn(move || {
            let result = apply_compact(op, &path, algorithm, allow, |progress| {
                let _ = tx.send_blocking(Ok(progress));
            });
            match result {
                Ok(done) => {
                    let _ = tx.send_blocking(Ok(CompactProgress {
                        processed: 1,
                        total: 1,
                        message: done.message,
                    }));
                }
                Err(err) => {
                    let _ = tx.send_blocking(Err(err));
                }
            }
        });

        cx.spawn(async move |this, cx| {
            while let Ok(item) = rx.recv().await {
                let cont = this.update(cx, |app, cx| match item {
                    Ok(progress) => {
                        let finished = progress.message.contains("WOF /EXE")
                            || progress.message == "Finished.";
                        app.compact_progress = Some(progress.clone());
                        if finished {
                            app.compact_busy = false;
                            app.show_toast(progress.message.clone(), cx);
                            notify_compact(
                                app.system_tray.as_ref(),
                                app.settings.os_notify_mode,
                                app.window_hidden_to_tray,
                                crate::branding::APP_NAME,
                                &format!("{name}: {}", progress.message),
                                true,
                            );
                            app.refresh_library(cx);
                        }
                        cx.notify();
                        true
                    }
                    Err(err) => {
                        app.compact_busy = false;
                        app.compact_progress = None;
                        app.show_error_toast(err.clone(), cx);
                        notify_compact(
                            app.system_tray.as_ref(),
                            app.settings.os_notify_mode,
                            app.window_hidden_to_tray,
                            crate::branding::APP_NAME,
                            &format!("{name}: {err}"),
                            false,
                        );
                        cx.notify();
                        false
                    }
                });
                if !matches!(cont, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    pub(crate) fn flush_state_now(&mut self) {
        let state = AppState {
            selected_app_id: self.selected_app_id,
            last_compact_app_id: self.selected_app_id,
        };
        let _ = save_state(&self.paths, &state);
    }

    pub(crate) fn flush_window_layout_now(&mut self) {
        if !self.window_layout_dirty {
            return;
        }
        self.window_layout_dirty = false;
        let _ = save_settings(&self.paths, &self.settings);
    }

    fn capture_window_layout(&mut self, window: &Window) {
        let layout = window_layout_from_window(window);
        if layout != self.settings.window_layout {
            self.settings.window_layout = layout;
            self.window_layout_dirty = true;
        }
        if self.window_layout_dirty && self.last_window_layout_save.elapsed().as_millis() > 750 {
            self.last_window_layout_save = Instant::now();
            self.flush_window_layout_now();
        }
    }
}

fn window_layout_from_window(window: &Window) -> WindowLayout {
    let wb = window.window_bounds();
    let bounds = wb.get_bounds();
    let mut layout = WindowLayout {
        width: bounds.size.width.to_f64() as f32,
        height: bounds.size.height.to_f64() as f32,
        x: Some(bounds.origin.x.to_f64() as f32),
        y: Some(bounds.origin.y.to_f64() as f32),
        maximized: matches!(wb, WindowBounds::Maximized(_)),
    };
    layout.sanitize();
    layout
}

fn is_compacted(game: &SteamGame) -> bool {
    match (game.on_disk_bytes, game.logical_bytes) {
        (Some(disk), Some(logical)) => disk + logical / 20 < logical,
        _ => false,
    }
}

impl Focusable for LibraryApp {
    fn focus_handle(&self, cx: &App) -> gpui::FocusHandle {
        self.search_input.focus_handle(cx)
    }
}

impl Render for LibraryApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.poll_hidden_window_actions(cx);
        self.apply_pending_tray_actions(window, cx);
        self.apply_pending_whats_new(window, cx);
        self.capture_window_layout(window);
        self.flush_toast(cx);

        if self.applied_window_transparency != Some(self.settings.window_transparency) {
            crate::appearance::apply_window_opacity(
                window,
                self.settings.window_transparency,
                self.settings.backdrop_blur,
            );
            self.applied_window_transparency = Some(self.settings.window_transparency);
        }

        let theme = cx.theme().clone();
        let noise = noise_enabled(self.settings.noise_intensity);
        let vignette = vignette_enabled(self.settings.vignette_intensity);
        let grain = noise.then(|| film_grain_image(self.settings.noise_intensity));
        let dialog_layer = Root::render_dialog_layer(window, cx);
        let sheet_layer = Root::render_sheet_layer(window, cx);

        div()
            .id("library-app-root")
            .relative()
            .size_full()
            .bg(theme.background)
            .child(
                v_flex().size_full().child(self.render_title_bar(cx)).child(
                    gpui_component::h_flex()
                        .flex_1()
                        .min_h_0()
                        .child(if self.filter == FilterKind::Settings {
                            self.render_settings_sidebar(cx).into_any_element()
                        } else {
                            self.render_sidebar(cx).into_any_element()
                        })
                        .child(if self.filter == FilterKind::Settings {
                            self.render_settings(cx).into_any_element()
                        } else {
                            self.render_library(cx).into_any_element()
                        }),
                ),
            )
            .when(vignette, |el| {
                el.child(render_vignette_overlay(
                    vignette_edge_alpha(self.settings.vignette_intensity),
                    theme.is_dark(),
                ))
            })
            .when_some(grain, |el, image| {
                el.child(
                    canvas(
                        |_bounds, _window, _cx| (),
                        move |bounds, (), window, _cx| {
                            let _ = window.paint_image(
                                bounds,
                                gpui::Corners::default(),
                                image.clone(),
                                0,
                                false,
                            );
                        },
                    )
                    .absolute()
                    .inset_0()
                    .size_full(),
                )
            })
            .children(dialog_layer)
            .children(sheet_layer)
            .child(self.render_toast_layer(cx))
    }
}
