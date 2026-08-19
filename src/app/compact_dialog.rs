//! Low / Medium / High compact dialog. No Switch — library Switch panics here.
//!
//! One ~480px dialog. Radio-cards show Low / Medium / High only — no XPRESS/LZX.

use gpui::{
    div, prelude::FluentBuilder, px, AppContext, Context, InteractiveElement, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex, v_flex, ActiveTheme, Icon, Sizable, StyledExt, WindowExt,
};

use super::LibraryApp;
use crate::compact::CompactLevel;

pub(crate) struct CompactLevelPicker {
    app: gpui::Entity<LibraryApp>,
    level: CompactLevel,
    heading: String,
}

impl CompactLevelPicker {
    fn new(app: gpui::Entity<LibraryApp>, heading: String) -> Self {
        Self {
            app,
            level: CompactLevel::Medium,
            heading,
        }
    }
}

impl Render for CompactLevelPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let selected = self.level;
        v_flex()
            .id("compact-level-picker")
            .gap_3()
            .w_full()
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(self.heading.clone()),
            )
            .child(
                v_flex()
                    .gap_2()
                    .w_full()
                    .children(CompactLevel::ALL.into_iter().map(|level| {
                        let on = selected == level;
                        h_flex()
                            .id(SharedString::from(format!(
                                "compact-level-{}",
                                level.label()
                            )))
                            .w_full()
                            .items_center()
                            .justify_between()
                            .px_3()
                            .py_3()
                            .gap_3()
                            .rounded(px(10.))
                            .border_1()
                            .border_color(if on {
                                theme.primary
                            } else {
                                theme.border.opacity(0.5)
                            })
                            .bg(if on {
                                theme.secondary.opacity(0.55)
                            } else {
                                theme.secondary.opacity(0.28)
                            })
                            .hover(|s| s.bg(theme.secondary.opacity(0.5)))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.level = level;
                                cx.notify();
                            }))
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        Icon::empty()
                                            .path(level.icon_path())
                                            .with_size(px(18.))
                                            .text_color(if on {
                                                theme.foreground
                                            } else {
                                                theme.muted_foreground
                                            }),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_semibold()
                                            .text_color(theme.foreground)
                                            .child(level.label()),
                                    )
                                    .when(level.recommended(), |el| {
                                        el.child(
                                            div()
                                                .px_1p5()
                                                .py_0p5()
                                                .rounded(px(4.))
                                                .bg(theme.muted.opacity(0.7))
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child("Recommended"),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(level.tradeoff()),
                            )
                    })),
            )
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("compact-level-confirm")
                            .primary()
                            .label("Compress")
                            .on_click({
                                let app = self.app.clone();
                                let level = self.level;
                                move |_, window, cx| {
                                    app.update(cx, |app, cx| {
                                        app.apply_compact_level(level, window, cx);
                                    });
                                    window.close_dialog(cx);
                                }
                            }),
                    )
                    .child(
                        Button::new("compact-level-cancel")
                            .outline()
                            .label("Cancel")
                            .on_click(|_, window, cx| {
                                window.close_dialog(cx);
                            }),
                    ),
            )
    }
}

impl LibraryApp {
    pub(crate) fn open_compact_level_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let titles = self.selected_titles();
        if titles.is_empty() {
            self.show_toast("Select a game first.", cx);
            return;
        }
        if self.compact_busy {
            self.show_toast("A compact job is already running.", cx);
            return;
        }
        let heading = if titles.len() == 1 {
            format!("Compress {}.", titles[0].name)
        } else {
            format!("Compress {} selected titles.", titles.len())
        };
        let app = cx.entity();
        let picker = cx.new(|_cx| CompactLevelPicker::new(app, heading));
        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme().clone();
            dialog
                .title("Compress")
                .overlay_closable(true)
                .keyboard(true)
                .on_cancel(|_, _, _| true)
                .w(px(480.))
                .border_color(theme.border.opacity(0.32))
                .child(picker.clone())
        });
    }
}
