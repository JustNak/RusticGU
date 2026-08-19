//! Small tray flyout near the notification icon.

use gpui::{
    div, point, prelude::FluentBuilder, px, size, App, AppContext, Bounds, Context,
    InteractiveElement, IntoElement, ParentElement, Styled, Window, WindowBounds,
    WindowDecorations, WindowKind, WindowOptions,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    v_flex, ActiveTheme, Disableable, Icon, IconName, Root, StyledExt, TitleBar,
};

use super::LibraryApp;
use crate::branding::APP_NAME;
use crate::format::{format_bytes, format_size_pair};

pub struct TrayFlyout {
    app: gpui::Entity<LibraryApp>,
}

impl TrayFlyout {
    fn new(app: gpui::Entity<LibraryApp>) -> Self {
        Self { app }
    }
}

impl gpui::Render for TrayFlyout {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let snapshot = self.app.read(cx).flyout_snapshot();
        let app = self.app.clone();

        v_flex()
            .id("tray-flyout")
            .w(px(320.))
            .p_3()
            .gap_3()
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_sm()
                    .font_semibold()
                    .text_color(theme.foreground)
                    .child(APP_NAME),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(format!(
                                "{} compact · {} inflated",
                                snapshot.compacted, snapshot.inflated
                            )),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(format!(
                                "Library {} · on disk {}",
                                format_bytes(snapshot.logical_bytes),
                                snapshot
                                    .on_disk_bytes
                                    .map(format_bytes)
                                    .unwrap_or_else(|| "—".into())
                            )),
                    )
                    .when_some(snapshot.selected_name.clone(), |el, name| {
                        el.child(div().text_xs().text_color(theme.foreground).child(format!(
                            "Selected: {name} · {}",
                            format_size_pair(snapshot.selected_logical, snapshot.selected_on_disk)
                        )))
                    }),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        Button::new("flyout-pause-live")
                            .outline()
                            .w_full()
                            .icon(if snapshot.live_paused {
                                Icon::empty().path("icons/play.svg")
                            } else {
                                Icon::new(IconName::Minus)
                            })
                            .label(if snapshot.live_paused {
                                "Resume live"
                            } else {
                                "Pause live"
                            })
                            .tooltip(if snapshot.live_paused {
                                "Live compact is paused. Click to resume."
                            } else {
                                "Pause live compact after patches."
                            })
                            .on_click({
                                let app = app.clone();
                                move |_, _, cx| {
                                    app.update(cx, |app, cx| {
                                        app.toggle_live_compact(cx);
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("flyout-recompact")
                            .primary()
                            .w_full()
                            .icon(IconName::Redo2)
                            .label("Retry last")
                            .disabled(!snapshot.has_last_plan || snapshot.compact_busy)
                            .tooltip("Retry compact on files from the last Steam patch.")
                            .on_click({
                                let app = app.clone();
                                move |_, _, cx| {
                                    app.update(cx, |app, cx| {
                                        app.recompact_last_patch(cx);
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("flyout-open-main")
                            .outline()
                            .w_full()
                            .icon(IconName::ExternalLink)
                            .label("Open RusticGU")
                            .on_click({
                                let app = app.clone();
                                move |_, _, cx| {
                                    app.update(cx, |app, cx| {
                                        app.restore_from_flyout(cx);
                                    });
                                }
                            }),
                    ),
            )
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FlyoutSnapshot {
    pub compacted: i32,
    pub inflated: i32,
    pub logical_bytes: u64,
    pub on_disk_bytes: Option<u64>,
    pub selected_name: Option<String>,
    pub selected_logical: Option<u64>,
    pub selected_on_disk: Option<u64>,
    pub live_paused: bool,
    pub has_last_plan: bool,
    pub compact_busy: bool,
}

impl LibraryApp {
    pub(crate) fn flyout_snapshot(&self) -> FlyoutSnapshot {
        let (_, compacted, uncompacted) = self.library_counts();
        let logical_bytes = self.games.iter().filter_map(|g| g.logical_bytes).sum();
        let on_disk: Option<u64> = {
            let disks: Vec<u64> = self.games.iter().filter_map(|g| g.on_disk_bytes).collect();
            if disks.is_empty() {
                None
            } else {
                Some(disks.into_iter().sum())
            }
        };
        let selected = self
            .selected_id
            .as_ref()
            .and_then(|id| self.games.iter().find(|g| g.id == *id));
        FlyoutSnapshot {
            compacted,
            inflated: uncompacted,
            logical_bytes,
            on_disk_bytes: on_disk,
            selected_name: selected.map(|g| g.name.clone()),
            selected_logical: selected.and_then(|g| g.logical_bytes),
            selected_on_disk: selected.and_then(|g| g.on_disk_bytes),
            live_paused: self.live.paused(),
            has_last_plan: self.live.last_plan().is_some(),
            compact_busy: self.compact_busy,
        }
    }

    pub(crate) fn toggle_flyout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.flyout_open {
            self.close_flyout(cx);
            return;
        }
        self.open_flyout(window, cx);
    }

    fn open_flyout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.flyout_open {
            return;
        }
        let app = cx.entity();
        let size = size(px(332.), px(260.));
        let origin = flyout_origin(window, cx);
        let bounds = Bounds { origin, size };
        let result = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some({
                    let mut opts = TitleBar::title_bar_options();
                    opts.title = Some("RusticGU tray".into());
                    opts
                }),
                window_decorations: Some(WindowDecorations::Client),
                kind: WindowKind::PopUp,
                ..Default::default()
            },
            move |window, cx| {
                let view = cx.new(|_cx| TrayFlyout::new(app));
                cx.new(|cx| Root::new(view, window, cx))
            },
        );
        if result.is_ok() {
            self.flyout_open = true;
        }
    }

    pub(crate) fn close_flyout(&mut self, _cx: &mut Context<Self>) {
        self.flyout_open = false;
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

fn flyout_origin(window: &Window, cx: &App) -> gpui::Point<gpui::Pixels> {
    let _ = window;
    if let Some(display) = cx.displays().first() {
        let bounds = display.bounds();
        return point(
            bounds.origin.x + bounds.size.width - px(348.),
            bounds.origin.y + bounds.size.height - px(320.),
        );
    }
    point(px(24.), px(24.))
}
