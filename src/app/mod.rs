mod about_dialog;
mod compact_apply;
mod compact_dialog;
mod compact_flow;
mod confirm_dialogs;
mod cover_flow;
mod filter;
mod inspector;
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

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use gpui::{
    canvas, div, prelude::FluentBuilder, AnyWindowHandle, App, AppContext, Context, Focusable,
    InteractiveElement, IntoElement, ParentElement, Render, Styled, Window, WindowBounds,
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
    apply_compact_force, estimate_compact, estimate_compact_with, CompactOp, CompactProgress,
};
use crate::library::{
    algorithm_from_policy, scan_library, shelf_policy_for, steam_title_id,
    title_is_compact_excluded, LibraryTitle, ScanOptions,
};
use crate::live::LiveHandle;
use crate::persistence::{
    load_pending_whats_new, load_state, save_settings, save_state, AppPaths, AppState,
    PendingWhatsNew,
};
use crate::settings::CompactAlgorithm;
use crate::settings::{Settings, WindowLayout};
use crate::startup::launched_minimized;
use crate::tray::SystemTray;
use crate::updater::UpdateInfo;
use compact_apply::{PosterJob, PosterJobKind};
use compact_flow::CompactFlow;
use toast::Toast;
use widgets::render_vignette_overlay;

pub struct LibraryApp {
    pub(crate) games: Vec<LibraryTitle>,
    pub(crate) settings: Settings,
    pub(crate) paths: AppPaths,
    pub(crate) filter: FilterKind,
    pub(crate) settings_return_filter: FilterKind,
    pub(crate) settings_category: SettingsCategory,
    pub(crate) selected_id: Option<String>,
    pub(crate) selected_ids: HashSet<String>,
    pub(crate) covers: HashMap<String, Arc<gpui::RenderImage>>,
    pub(crate) cover_inflight: HashSet<String>,
    pub(crate) library_scanning: bool,
    pub(crate) library_error: Option<String>,
    pub(crate) compact_busy: bool,
    pub(crate) compact_progress: Option<CompactProgress>,
    pub(crate) poster_job: Option<PosterJob>,
    pub(crate) compact_flow: Option<CompactFlow>,
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
    pub(crate) pending_open_compact: bool,
    pub(crate) flyout_open: bool,
    pub(crate) flyout_window: Option<AnyWindowHandle>,
    pub(crate) activate: ActivateBridge,
    pub(crate) live: LiveHandle,
    pub(crate) last_patching: HashSet<String>,
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
            selected_id: state
                .selected_title_id
                .or_else(|| state.selected_app_id.map(steam_title_id)),
            selected_ids: HashSet::new(),
            covers: HashMap::new(),
            cover_inflight: HashSet::new(),
            library_scanning: true,
            library_error: None,
            compact_busy: false,
            compact_progress: None,
            poster_job: None,
            compact_flow: None,
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
            pending_open_compact: false,
            flyout_open: false,
            flyout_window: None,
            activate,
            live: LiveHandle::start(),
            last_patching: HashSet::new(),
        };
        if let Some(id) = app.selected_id.clone() {
            app.selected_ids.insert(id);
        }
        app.live
            .set_allow_dstorage(app.settings.allow_dstorage_override);
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
            let options = this
                .update(cx, |app, _| ScanOptions {
                    include_xbox_games: app.settings.include_xbox_games,
                    custom_directories: app.settings.custom_game_directories.clone(),
                })
                .unwrap_or_default();
            let result = cx
                .background_executor()
                .spawn(async move { scan_library(options) })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.library_scanning = false;
                match result {
                    Ok(mut games) => {
                        crate::covers::attach_itch_cover_urls(
                            &mut games,
                            crate::library::extra_store_roots(false)
                                .itch_config
                                .as_deref(),
                        );
                        app.games = games;
                        app.library_error = None;
                        app.prune_selection();
                        if app.selected_id.is_none() {
                            if let Some(first) = app.games.first() {
                                app.selected_id = Some(first.id.clone());
                                app.selected_ids.insert(first.id.clone());
                            }
                        }
                        app.live.sync_titles(&app.games);
                        app.live
                            .set_allow_dstorage(app.settings.allow_dstorage_override);
                        app.drop_missing_store_filter();
                        app.hydrate_covers_from_disk();
                        app.request_covers(cx);
                    }
                    Err(msg) => {
                        app.games.clear();
                        app.library_error = Some(msg);
                        app.drop_missing_store_filter();
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
        self.drop_missing_store_filter();
        cx.notify();
    }

    pub(crate) fn select_game(&mut self, id: String, cx: &mut Context<Self>) {
        self.selected_ids.clear();
        self.selected_ids.insert(id.clone());
        self.selected_id = Some(id);
        self.flush_state_now();
        cx.notify();
    }

    pub(crate) fn select_game_click(&mut self, id: String, multi: bool, cx: &mut Context<Self>) {
        if multi {
            if self.selected_ids.contains(&id) {
                self.selected_ids.remove(&id);
                if self.selected_id.as_deref() == Some(id.as_str()) {
                    self.selected_id = self.selected_ids.iter().next().cloned();
                }
            } else {
                self.selected_ids.insert(id.clone());
                self.selected_id = Some(id);
            }
        } else {
            self.selected_ids.clear();
            self.selected_ids.insert(id.clone());
            self.selected_id = Some(id);
        }
        self.flush_state_now();
        cx.notify();
    }

    pub(crate) fn prune_selection(&mut self) {
        let known: HashSet<String> = self.games.iter().map(|g| g.id.clone()).collect();
        self.selected_ids.retain(|id| known.contains(id));
        if let Some(id) = self.selected_id.clone() {
            if !known.contains(&id) {
                self.selected_id = self.selected_ids.iter().next().cloned();
            }
        }
        if self.selected_id.is_none() {
            self.selected_id = self.selected_ids.iter().next().cloned();
        }
    }

    pub(crate) fn selected_titles(&self) -> Vec<LibraryTitle> {
        let mut titles: Vec<LibraryTitle> = self
            .games
            .iter()
            .filter(|g| self.selected_ids.contains(&g.id))
            .cloned()
            .collect();
        if titles.is_empty() {
            if let Some(game) = self.selected_game() {
                titles.push(game.clone());
            }
        }
        titles
    }

    pub(crate) fn cover_image(&self, id: &str) -> Option<Arc<gpui::RenderImage>> {
        self.covers.get(id).cloned()
    }

    pub(crate) fn open_install_folder(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(game) = self.games.iter().find(|g| g.id == id) else {
            self.show_toast("Select a game first.", cx);
            return;
        };
        if let Err(err) = open::that(&game.install_path) {
            self.show_error_toast(format!("Could not open folder: {err}"), cx);
        }
    }

    pub(crate) fn library_counts(&self) -> (i32, i32, i32) {
        let all = self.games.len() as i32;
        let compacted = self.games.iter().filter(|g| g.is_compacted()).count() as i32;
        (all, compacted, all - compacted)
    }

    fn drop_missing_store_filter(&mut self) {
        self.filter = filter::fallback_missing_store(self.filter, &self.games);
        self.settings_return_filter =
            filter::fallback_missing_store(self.settings_return_filter, &self.games);
    }

    pub(crate) fn visible_games(&self, cx: &App) -> Vec<LibraryTitle> {
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
                FilterKind::Store(store) => game.store == store,
                FilterKind::Compacted => game.is_compacted(),
                FilterKind::Uncompacted => !game.is_compacted(),
            })
            .filter(|game| {
                query.is_empty()
                    || game.name.to_ascii_lowercase().contains(&query)
                    || game.id.to_ascii_lowercase().contains(&query)
                    || game.store.badge().to_ascii_lowercase().contains(&query)
                    || game
                        .launcher_id
                        .as_deref()
                        .is_some_and(|id| id.to_ascii_lowercase().contains(&query))
            })
            .cloned()
            .collect()
    }

    pub(crate) fn selected_game(&self) -> Option<&LibraryTitle> {
        let id = self.selected_id.as_ref()?;
        self.games.iter().find(|g| g.id == *id)
    }

    fn compact_algorithm_for(
        &self,
        title: &LibraryTitle,
        is_launching: bool,
    ) -> Option<CompactAlgorithm> {
        let policy = shelf_policy_for(title, is_launching);
        algorithm_from_policy(&policy, self.settings.compact_algorithm)
    }

    pub(crate) fn estimate_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(game) = self.selected_game().cloned() else {
            self.show_toast("Select a game first.", cx);
            return;
        };
        if title_is_compact_excluded(&game) {
            self.show_error_toast(format!("{} is auto-excluded from compact.", game.name), cx);
            return;
        }
        let Some(algorithm) = self.compact_algorithm_for(&game, false) else {
            self.show_error_toast(format!("{} is auto-excluded from compact.", game.name), cx);
            return;
        };
        let estimate = if algorithm == CompactAlgorithm::Lzx {
            estimate_compact_with(&game.install_path, algorithm)
        } else {
            estimate_compact(&game.install_path, algorithm)
        };
        match estimate {
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
        match op {
            CompactOp::Compress => self.open_compact_flow(window, cx),
            CompactOp::Uncompress => {
                self.apply_compact_jobs(self.selected_titles(), op, None, false, window, cx)
            }
        }
    }

    pub(crate) fn flush_state_now(&mut self) {
        let selected_steam = self.selected_game().and_then(|g| g.steam_app_id());
        let state = AppState {
            selected_app_id: selected_steam,
            selected_title_id: self.selected_id.clone(),
            last_compact_app_id: selected_steam,
        };
        let _ = save_state(&self.paths, &state);
    }

    fn sync_patching_titles(&mut self, cx: &mut Context<Self>) {
        let next: HashSet<String> = self
            .games
            .iter()
            .filter(|game| {
                game.steam_app_id()
                    .is_some_and(|id| self.live.is_locked(&id.to_string()))
            })
            .map(|game| game.id.clone())
            .collect();
        if next != self.last_patching {
            self.last_patching = next;
            cx.notify();
        }
    }

    pub(crate) fn toggle_live_compact(&mut self, cx: &mut Context<Self>) {
        let paused = self.live.toggle_paused();
        self.show_toast(
            if paused {
                "Paused live."
            } else {
                "Resumed live."
            },
            cx,
        );
        cx.notify();
    }

    pub(crate) fn recompact_last_patch(&mut self, cx: &mut Context<Self>) {
        if self.compact_busy {
            self.show_toast("A compact job is already running.", cx);
            return;
        }
        self.compact_busy = true;
        self.live.set_compact_busy(true);
        self.compact_progress = None;
        if let Some(plan) = self.live.last_plan() {
            if let Some(game) = self
                .games
                .iter()
                .find(|g| g.install_path == plan.install)
                .cloned()
            {
                self.poster_job = PosterJob::for_titles(
                    std::slice::from_ref(&game),
                    PosterJobKind::Compress,
                    "Retrying last patch…",
                );
            }
        }
        cx.notify();
        let live = self.live.clone();
        let (tx, rx) = async_channel::unbounded::<Result<String, String>>();
        std::thread::spawn(move || {
            let _ = tx.send_blocking(live.recompact_last_patch());
        });
        cx.spawn(async move |this, cx| {
            if let Ok(result) = rx.recv().await {
                let _ = this.update(cx, |app, cx| {
                    app.compact_busy = false;
                    app.live.set_compact_busy(false);
                    app.compact_progress = None;
                    app.poster_job = None;
                    match result {
                        Ok(msg) => {
                            app.show_toast(msg, cx);
                            app.refresh_library(cx);
                        }
                        Err(err) => app.show_error_toast(err, cx),
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub(crate) fn launch_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(game) = self.selected_game().cloned() else {
            self.show_toast("Select a game first.", cx);
            return;
        };
        let policy = shelf_policy_for(&game, true);
        if matches!(policy, shelf::CompactPolicy::Xpress) && !self.compact_busy {
            self.start_compact_walkback_then_launch(&game, window, cx);
            return;
        }
        if let Err(msg) = open_title(&game) {
            self.show_error_toast(msg, cx);
        }
    }

    fn start_compact_walkback_then_launch(
        &mut self,
        game: &LibraryTitle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = window;
        if title_is_compact_excluded(game) {
            if let Err(msg) = open_title(game) {
                self.show_error_toast(msg, cx);
            }
            return;
        }
        self.compact_busy = true;
        self.live.set_compact_busy(true);
        self.compact_progress = None;
        self.poster_job = PosterJob::for_titles(
            std::slice::from_ref(game),
            PosterJobKind::Walkback,
            "Walking back to XPRESS…",
        );
        cx.notify();
        let allow = self.settings.allow_dstorage_override;
        let path = game.install_path.clone();
        let name = game.name.clone();
        let launch_game = game.clone();
        let (tx, rx) = async_channel::unbounded::<Result<CompactProgress, String>>();
        std::thread::spawn(move || {
            let result = apply_compact_force(
                CompactOp::Compress,
                &path,
                CompactAlgorithm::Xpress,
                allow,
                |progress| {
                    let _ = tx.send_blocking(Ok(progress));
                },
            );
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
                        if let Some(job) = app.poster_job.as_mut() {
                            job.progress = progress;
                        }
                        if finished {
                            app.compact_busy = false;
                            app.live.set_compact_busy(false);
                            app.poster_job = None;
                            app.compact_progress = None;
                            if let Err(msg) = open_title(&launch_game) {
                                app.show_error_toast(msg, cx);
                            } else {
                                app.show_toast(format!("{name}: walked back to XPRESS."), cx);
                            }
                        }
                        cx.notify();
                        true
                    }
                    Err(err) => {
                        app.compact_busy = false;
                        app.live.set_compact_busy(false);
                        app.compact_progress = None;
                        app.poster_job = None;
                        if let Err(msg) = open_title(&launch_game) {
                            app.show_error_toast(format!("{err}: {msg}"), cx);
                        } else {
                            app.show_error_toast(err, cx);
                        }
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

fn open_title(title: &LibraryTitle) -> Result<(), String> {
    if let Some(app_id) = title.steam_app_id() {
        open::that(format!("steam://rungameid/{app_id}"))
            .map_err(|e| format!("Could not launch Steam title: {e}"))
    } else {
        open::that(&title.install_path).map_err(|e| format!("Could not open install folder: {e}"))
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
        self.sync_patching_titles(cx);
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
            .when(self.compact_flow.is_some(), |el| {
                el.child(self.render_compact_flow(cx))
            })
    }
}
