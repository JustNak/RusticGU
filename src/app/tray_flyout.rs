//! Compact Proton-style tray panel: header + one primary + list + footer.
//!
//! QA FAIL #1 — chrome-less: `WindowKind::PopUp`, `titlebar: None`. Do not import
//! or apply client title-bar options. The panel paints its own header.
//! QA FAIL #2 — place from `Shell_NotifyIconGetRect` + work area (see
//! `window_placement` / `tray::anchor_from_notify_rect`).
//! QA FAIL #3 — footer **Open RusticGU** + **Exit** on the panel; Exit calls
//! `force_quit_app` (not tray-menu `ID_TRAY_EXIT` only).
//!
//! Do not wrap in `gpui_component::Root` — Root's `window_border` paints a
//! transparent backdrop. Open from the tray event (never `LibraryApp::render`).

use std::time::Instant;

use gpui::{
    div, hsla, img, prelude::FluentBuilder, px, size, AppContext, Bounds, Context,
    InteractiveElement, IntoElement, ObjectFit, ParentElement, SharedString, Size, Styled,
    StyledImage, Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex, v_flex, ActiveTheme, Disableable, Icon, StyledExt,
};

use super::LibraryApp;
use crate::appearance::apply_window_opacity;
use crate::branding::{APP_LOGO_DARK, APP_NAME};
use crate::library::LibraryTitle;
use crate::window_placement::{
    place_flyout_above_tray, FLYOUT_HEIGHT_PX, FLYOUT_TASKBAR_CLEARANCE_PX, FLYOUT_WIDTH_PX,
};

/// Always-dark panel tokens (Proton-style). Never inherit the main title bar.
fn panel_bg() -> gpui::Hsla {
    hsla(0.53, 0.16, 0.075, 1.0)
}
fn panel_border() -> gpui::Hsla {
    hsla(0.53, 0.10, 0.20, 1.0)
}
fn panel_fg() -> gpui::Hsla {
    hsla(0.53, 0.04, 0.94, 1.0)
}
fn panel_muted() -> gpui::Hsla {
    hsla(0.53, 0.08, 0.62, 1.0)
}
fn panel_row() -> gpui::Hsla {
    hsla(0.53, 0.12, 0.11, 1.0)
}
fn panel_live() -> gpui::Hsla {
    hsla(0.50, 0.55, 0.52, 1.0)
}

const FLYOUT_W: f32 = FLYOUT_WIDTH_PX as f32;
const FLYOUT_H: f32 = FLYOUT_HEIGHT_PX as f32;

pub struct TrayFlyout {
    app: gpui::Entity<LibraryApp>,
    opened_at: Instant,
}

impl TrayFlyout {
    fn new(app: gpui::Entity<LibraryApp>) -> Self {
        Self {
            app,
            opened_at: Instant::now(),
        }
    }
}

impl gpui::Render for TrayFlyout {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Never inherit the main window's layered glass — this panel must be solid.
        window.set_background_appearance(WindowBackgroundAppearance::Opaque);
        apply_window_opacity(window, 0, false);

        let theme = cx.theme().clone();
        let snapshot = self.app.read(cx).flyout_snapshot();
        let app = self.app.clone();
        let fg = panel_fg();
        let muted = panel_muted();
        let live = if theme.is_dark() {
            theme.primary
        } else {
            panel_live()
        };

        div()
            .id("tray-flyout")
            .size_full()
            .flex()
            .flex_col()
            .bg(panel_bg())
            .text_color(fg)
            .border_1()
            .border_color(panel_border())
            .rounded(px(12.))
            .p_3()
            .child(
                v_flex()
                    .id("tray-flyout-body")
                    .size_full()
                    .gap_2()
                    .child(render_header(&snapshot, live, fg, muted))
                    .child(section_rule())
                    .child(render_primary(&snapshot, app.clone()))
                    .child(section_rule())
                    .child(render_list(&snapshot, app.clone(), fg, muted))
                    .child(section_rule())
                    .child(render_footer(app, muted)),
            )
    }
}

fn section_rule() -> impl IntoElement {
    div()
        .id("tray-flyout-rule")
        .w_full()
        .h(px(1.))
        .bg(panel_border())
}

fn render_header(
    snapshot: &FlyoutSnapshot,
    live_color: gpui::Hsla,
    fg: gpui::Hsla,
    muted: gpui::Hsla,
) -> impl IntoElement {
    v_flex()
        .id("tray-flyout-header")
        .w_full()
        .gap_1()
        .child(
            h_flex()
                .id("tray-flyout-brand")
                .items_center()
                .gap_2()
                .child(
                    img(APP_LOGO_DARK)
                        .w(px(20.))
                        .h(px(20.))
                        .object_fit(ObjectFit::Contain),
                )
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .text_color(fg)
                        .child(APP_NAME),
                ),
        )
        .child(div().text_xs().text_color(muted).child(format!(
            "{} compact · {} inflated",
            snapshot.compacted, snapshot.inflated
        )))
        .child(
            h_flex()
                .id("tray-flyout-live")
                .items_center()
                .gap_1()
                .child(
                    Icon::empty()
                        .path(if snapshot.live_paused {
                            "icons/minus.svg"
                        } else {
                            "icons/circle-check.svg"
                        })
                        .text_color(if snapshot.live_paused {
                            muted
                        } else {
                            live_color
                        }),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(if snapshot.live_paused { muted } else { fg })
                        .child(if snapshot.live_paused {
                            "Live Compact paused"
                        } else {
                            "Live Compact on"
                        }),
                ),
        )
}

fn render_primary(snapshot: &FlyoutSnapshot, app: gpui::Entity<LibraryApp>) -> impl IntoElement {
    match snapshot.primary {
        FlyoutPrimary::Resume => Button::new("flyout-primary")
            .primary()
            .w_full()
            .icon(Icon::empty().path("icons/play.svg"))
            .label("Resume")
            .tooltip("Resume live compact after patches.")
            .on_click(move |_, _, cx| {
                app.update(cx, |app, cx| {
                    app.toggle_live_compact(cx);
                    app.refresh_flyout(cx);
                });
            }),
        FlyoutPrimary::Compress => Button::new("flyout-primary")
            .primary()
            .w_full()
            .icon(Icon::empty().path("icons/file-archive.svg"))
            .label("Compress")
            .disabled(snapshot.compact_busy || snapshot.inflated == 0)
            .tooltip("Open the main window and pick Low / Medium / High.")
            .on_click(move |_, _, cx| {
                app.update(cx, |app, cx| {
                    app.request_compress_from_flyout(cx);
                });
            }),
    }
}

fn render_list(
    snapshot: &FlyoutSnapshot,
    app: gpui::Entity<LibraryApp>,
    fg: gpui::Hsla,
    muted: gpui::Hsla,
) -> impl IntoElement {
    let items = snapshot.list.clone();
    v_flex()
        .id("tray-flyout-list")
        .w_full()
        .flex_1()
        .min_h(px(88.))
        .gap_1()
        .when(items.is_empty(), |el| {
            el.child(div().text_xs().text_color(muted).child("No titles yet."))
        })
        .children(items.into_iter().enumerate().map(|(i, item)| {
            let app = app.clone();
            let can_retry = item.kind == FlyoutListKind::LastPatch && snapshot.has_last_plan;
            let busy = snapshot.compact_busy;
            h_flex()
                .id(SharedString::from(format!("flyout-row-{i}")))
                .w_full()
                .items_center()
                .justify_between()
                .gap_2()
                .px_2()
                .py_1()
                .rounded(px(8.))
                .bg(panel_row())
                .child(
                    v_flex()
                        .min_w_0()
                        .flex_1()
                        .child(div().text_xs().text_color(fg).child(item.name.clone()))
                        .when(item.kind == FlyoutListKind::LastPatch, |el| {
                            el.child(div().text_xs().text_color(muted).child("Last patch"))
                        }),
                )
                .when(can_retry, move |el| {
                    el.child(
                        Button::new("flyout-retry-last")
                            .ghost()
                            .compact()
                            .icon(Icon::empty().path("icons/redo-2.svg"))
                            .label("Retry last")
                            .disabled(busy)
                            .on_click(move |_, _, cx| {
                                app.update(cx, |app, cx| {
                                    app.recompact_last_patch(cx);
                                    app.refresh_flyout(cx);
                                });
                            }),
                    )
                })
        }))
}

/// Footer buttons painted on the flyout itself (QA FAIL #3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlyoutFooterItem {
    OpenRusticGu,
    Exit,
}

/// What the footer Exit button does — full process quit, same as `ID_TRAY_EXIT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlyoutExitCommand {
    ForceQuit,
}

pub(crate) fn flyout_footer_items() -> [FlyoutFooterItem; 2] {
    [FlyoutFooterItem::OpenRusticGu, FlyoutFooterItem::Exit]
}

pub(crate) fn flyout_footer_label(item: FlyoutFooterItem) -> &'static str {
    match item {
        FlyoutFooterItem::OpenRusticGu => "Open RusticGU",
        FlyoutFooterItem::Exit => "Exit",
    }
}

pub(crate) fn flyout_exit_command() -> FlyoutExitCommand {
    FlyoutExitCommand::ForceQuit
}

fn render_footer(app: gpui::Entity<LibraryApp>, muted: gpui::Hsla) -> impl IntoElement {
    h_flex()
        .id("tray-flyout-footer")
        .w_full()
        .items_center()
        .justify_between()
        .gap_2()
        .child(
            Button::new("flyout-open-main")
                .ghost()
                .compact()
                .text_color(muted)
                .icon(Icon::empty().path("icons/external-link.svg"))
                .label(flyout_footer_label(FlyoutFooterItem::OpenRusticGu))
                .on_click({
                    let app = app.clone();
                    move |_, _, cx| {
                        app.update(cx, |app, cx| {
                            app.restore_from_flyout(cx);
                        });
                    }
                }),
        )
        .child(
            Button::new("flyout-exit")
                .ghost()
                .compact()
                .text_color(muted)
                .icon(Icon::empty().path("icons/window-close.svg"))
                .label(flyout_footer_label(FlyoutFooterItem::Exit))
                .on_click(move |_, _, cx| {
                    debug_assert_eq!(flyout_exit_command(), FlyoutExitCommand::ForceQuit);
                    app.update(cx, |app, cx| {
                        app.force_quit_app(cx);
                    });
                }),
        )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlyoutPrimary {
    Compress,
    Resume,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlyoutListKind {
    Title,
    LastPatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FlyoutListItem {
    pub name: String,
    pub kind: FlyoutListKind,
}

#[derive(Debug, Clone)]
pub(crate) struct FlyoutSnapshot {
    pub compacted: i32,
    pub inflated: i32,
    pub live_paused: bool,
    pub has_last_plan: bool,
    pub compact_busy: bool,
    pub primary: FlyoutPrimary,
    pub list: Vec<FlyoutListItem>,
}

impl LibraryApp {
    pub(crate) fn flyout_snapshot(&self) -> FlyoutSnapshot {
        let (_, compacted, uncompacted) = self.library_counts();
        let last_patch = last_patch_name(&self.games, self.live.last_plan().as_ref());
        FlyoutSnapshot {
            compacted,
            inflated: uncompacted,
            live_paused: self.live.paused(),
            has_last_plan: self.live.last_plan().is_some(),
            compact_busy: self.compact_busy,
            primary: flyout_primary(self.live.paused()),
            list: flyout_list_items(&self.games, last_patch, 3),
        }
    }

    pub(crate) fn toggle_flyout(&mut self, cx: &mut Context<Self>) {
        if self.flyout_open {
            self.close_flyout(cx);
            return;
        }
        self.open_flyout(cx);
    }

    fn open_flyout(&mut self, cx: &mut Context<Self>) {
        if self.flyout_open {
            return;
        }
        let app = cx.entity();
        let size = size(px(FLYOUT_W), px(FLYOUT_H));
        let origin = crate::window_placement::flyout_fallback_origin(cx);
        let bounds = Bounds { origin, size };
        let result = cx.open_window(flyout_window_options(bounds, size), {
            let app = app.clone();
            move |window, cx| {
                window.set_background_appearance(WindowBackgroundAppearance::Opaque);
                apply_window_opacity(window, 0, false);
                let view = cx.new(|_cx| TrayFlyout::new(app.clone()));
                window.on_window_should_close(cx, {
                    let app = app.clone();
                    move |_, cx| {
                        app.update(cx, |app, _| {
                            app.flyout_open = false;
                            app.flyout_window = None;
                        });
                        true
                    }
                });
                view.update(cx, |_, cx| {
                    cx.observe_window_activation(window, |this, window, cx| {
                        if window.is_window_active() {
                            return;
                        }
                        if this.opened_at.elapsed().as_millis() < 250 {
                            return;
                        }
                        this.app.update(cx, |app, cx| {
                            app.close_flyout(cx);
                        });
                    })
                    .detach();
                });
                view
            }
        });
        match result {
            Ok(handle) => {
                self.flyout_window = Some(*handle);
                self.flyout_open = true;
                let anchor = self
                    .system_tray
                    .as_ref()
                    .and_then(|tray| tray.icon_anchor())
                    .or_else(crate::tray::cursor_anchor);
                let _ = handle.update(cx, |_, window, _cx| {
                    window.set_background_appearance(WindowBackgroundAppearance::Opaque);
                    apply_window_opacity(window, 0, false);
                    place_flyout_above_tray(
                        window,
                        anchor,
                        FLYOUT_WIDTH_PX,
                        FLYOUT_HEIGHT_PX,
                        FLYOUT_TASKBAR_CLEARANCE_PX,
                    );
                    window.activate_window();
                    window.refresh();
                });
            }
            Err(err) => {
                eprintln!("[rusticgu] tray flyout: {err}");
                self.flyout_open = false;
                self.flyout_window = None;
            }
        }
    }

    pub(crate) fn close_flyout(&mut self, cx: &mut Context<Self>) {
        self.flyout_open = false;
        if let Some(handle) = self.flyout_window.take() {
            let _ = cx.update_window(handle, |_, window, _| {
                window.remove_window();
            });
        }
    }

    pub(crate) fn refresh_flyout(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.flyout_window.and_then(|h| h.downcast::<TrayFlyout>()) {
            let _ = handle.update(cx, |_, _, cx| cx.notify());
        }
        cx.notify();
    }

    pub(crate) fn request_compress_from_flyout(&mut self, cx: &mut Context<Self>) {
        if self.selected_titles().is_empty() {
            if let Some(game) = self.games.iter().find(|g| !g.is_compacted()) {
                self.selected_id = Some(game.id.clone());
                self.selected_ids.clear();
                self.selected_ids.insert(game.id.clone());
            }
        }
        self.pending_open_compact = true;
        self.pending_tray_show = true;
        self.restore_main_window_now_pub();
        self.close_flyout(cx);
        cx.notify();
    }

    pub(crate) fn restore_from_flyout(&mut self, cx: &mut Context<Self>) {
        self.restore_main_window_now_pub();
        self.pending_tray_show = true;
        self.close_flyout(cx);
        cx.notify();
    }

    pub(crate) fn restore_main_window_now_pub(&mut self) {
        self.window_hidden_to_tray = false;
        if self.main_hwnd != 0 {
            crate::tray::show_main_window_hwnd(self.main_hwnd);
        }
    }
}

/// Caption-free popup options.
///
/// QA FAIL #1: `titlebar: None`. Client decorations stay off. The painted
/// header is the only chrome.
pub(crate) fn flyout_window_options(
    bounds: Bounds<gpui::Pixels>,
    size: Size<gpui::Pixels>,
) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        window_decorations: None,
        window_background: WindowBackgroundAppearance::Opaque,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        focus: true,
        show: true,
        window_min_size: Some(size),
        ..Default::default()
    }
}

pub(crate) fn flyout_primary(live_paused: bool) -> FlyoutPrimary {
    if live_paused {
        FlyoutPrimary::Resume
    } else {
        FlyoutPrimary::Compress
    }
}

pub(crate) fn last_patch_name(
    games: &[LibraryTitle],
    plan: Option<&crate::live::StoredPlan>,
) -> Option<String> {
    let plan = plan?;
    if let Some(game) = games.iter().find(|g| {
        g.steam_app_id()
            .is_some_and(|id| id.to_string() == plan.plan.title_id)
    }) {
        return Some(game.name.clone());
    }
    plan.install
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
}

pub(crate) fn flyout_list_items(
    games: &[LibraryTitle],
    last_patch: Option<String>,
    limit: usize,
) -> Vec<FlyoutListItem> {
    let mut out = Vec::new();
    if let Some(name) = last_patch {
        out.push(FlyoutListItem {
            name,
            kind: FlyoutListKind::LastPatch,
        });
    }
    for game in games.iter().filter(|g| !g.is_compacted()) {
        if out.iter().any(|item| item.name == game.name) {
            continue;
        }
        out.push(FlyoutListItem {
            name: game.name.clone(),
            kind: FlyoutListKind::Title,
        });
        if out.len() >= limit {
            return out;
        }
    }
    for game in games {
        if out.iter().any(|item| item.name == game.name) {
            continue;
        }
        out.push(FlyoutListItem {
            name: game.name.clone(),
            kind: FlyoutListKind::Title,
        });
        if out.len() >= limit {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{LibraryStore, LibraryTitle};
    use crate::live::StoredPlan;
    use std::path::PathBuf;
    use watch::IncrementalPlan;

    fn title(name: &str, compacted: bool, app_id: Option<u32>) -> LibraryTitle {
        LibraryTitle {
            id: format!("steam:{}", app_id.unwrap_or(0)),
            name: name.into(),
            install_path: PathBuf::from(format!("C:\\games\\{name}")),
            store: LibraryStore::Steam,
            launcher_id: None,
            last_played_unix: None,
            logical_bytes: if compacted { Some(10) } else { Some(20) },
            on_disk_bytes: if compacted { Some(4) } else { Some(20) },
            compacted,
            steam_app_id: app_id,
            steam_library_path: None,
            steam_install_dir_name: None,
            cover_url: None,
        }
    }

    #[test]
    fn primary_is_resume_when_live_paused() {
        assert_eq!(flyout_primary(true), FlyoutPrimary::Resume);
        assert_eq!(flyout_primary(false), FlyoutPrimary::Compress);
    }

    #[test]
    fn list_leads_with_last_patch_then_inflated() {
        let games = vec![
            title("Compacted One", true, Some(1)),
            title("Elden Ring", false, Some(2)),
            title("Hades", false, Some(3)),
        ];
        let items = flyout_list_items(&games, Some("Cyberpunk".into()), 3);
        assert_eq!(items[0].kind, FlyoutListKind::LastPatch);
        assert_eq!(items[0].name, "Cyberpunk");
        assert_eq!(items[1].name, "Elden Ring");
        assert_eq!(items[2].name, "Hades");
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn last_patch_resolves_steam_title() {
        let games = vec![title("Hades", false, Some(1145360))];
        let plan = StoredPlan {
            plan: IncrementalPlan {
                title_id: "1145360".into(),
                files: vec![],
            },
            install: PathBuf::from("C:\\Steam\\steamapps\\common\\Hades"),
        };
        assert_eq!(
            last_patch_name(&games, Some(&plan)).as_deref(),
            Some("Hades")
        );
    }

    #[test]
    fn empty_library_has_empty_list() {
        assert!(flyout_list_items(&[], None, 3).is_empty());
    }

    #[test]
    fn flyout_window_has_no_titlebar_chrome() {
        let size = size(px(320.), px(412.));
        let bounds = Bounds {
            origin: gpui::point(px(0.), px(0.)),
            size,
        };
        let opts = flyout_window_options(bounds, size);
        assert!(
            opts.titlebar.is_none(),
            "QA FAIL #1: no TitleBar / title_bar_options"
        );
        assert!(
            opts.window_decorations.is_none(),
            "no client-decorated chrome"
        );
        assert_eq!(opts.kind, WindowKind::PopUp);
        assert!(!opts.is_resizable);
        assert!(!opts.is_movable);
        assert_eq!(opts.window_background, WindowBackgroundAppearance::Opaque);
    }

    #[test]
    fn flyout_source_never_uses_titlebar_chrome() {
        let src = include_str!("tray_flyout.rs");
        let prod = src.split("#[cfg(test)]").next().expect("prod source");
        assert!(
            !prod.contains("TitleBar::"),
            "must not call TitleBar::title_bar_options()"
        );
        assert!(
            !prod.contains("title_bar_options"),
            "must not pass TitleBar::title_bar_options()"
        );
        assert!(
            !prod.contains("RusticGU tray"),
            "must not set title \"RusticGU tray\""
        );
        assert!(
            !prod.contains("px(348"),
            "must not hardcode display width-348"
        );
    }

    #[test]
    fn flyout_footer_has_open_and_exit() {
        let items = flyout_footer_items();
        assert_eq!(items[0], FlyoutFooterItem::OpenRusticGu);
        assert_eq!(items[1], FlyoutFooterItem::Exit);
        assert_eq!(
            flyout_footer_label(FlyoutFooterItem::OpenRusticGu),
            "Open RusticGU"
        );
        assert_eq!(flyout_footer_label(FlyoutFooterItem::Exit), "Exit");
        assert_eq!(
            flyout_exit_command(),
            FlyoutExitCommand::ForceQuit,
            "QA FAIL #3: panel Exit must quit the app, not only ID_TRAY_EXIT"
        );
        let src = include_str!("tray_flyout.rs");
        assert!(src.contains("force_quit_app"));
        assert!(src.contains("flyout-exit"));
    }
}
