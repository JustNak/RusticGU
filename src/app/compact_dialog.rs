//! Low / Medium / High compact dialog. No Switch — library Switch panics here.

use gpui::{
    div, px, AppContext, Context, InteractiveElement, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex, v_flex, ActiveTheme, StyledExt, WindowExt,
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
            .w(px(620.))
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(self.heading.clone()),
            )
            .child(
                h_flex()
                    .gap_3()
                    .w_full()
                    .children(CompactLevel::ALL.into_iter().map(|level| {
                        let on = selected == level;
                        v_flex()
                            .id(SharedString::from(format!(
                                "compact-level-{}",
                                level.label()
                            )))
                            .flex_1()
                            .min_h(px(132.))
                            .p_3()
                            .gap_2()
                            .rounded(theme.radius_lg)
                            .border_1()
                            .border_color(if on {
                                theme.list_active_border
                            } else {
                                theme.border.opacity(0.55)
                            })
                            .bg(if on {
                                theme.list_active
                            } else {
                                theme.secondary.opacity(0.35)
                            })
                            .hover(|s| s.bg(theme.secondary.opacity(0.55)))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.level = level;
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .text_lg()
                                    .font_bold()
                                    .text_color(if on { theme.primary } else { theme.foreground })
                                    .child(level.label()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(level.tradeoff()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground.opacity(0.8))
                                    .child(level.algorithm().label()),
                            )
                    })),
            )
            .child(
                h_flex().w_full().justify_end().child(
                    Button::new("compact-level-confirm")
                        .primary()
                        .label("Confirm")
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
            format!("Compact {}.", titles[0].name)
        } else {
            format!("Compact {} selected titles.", titles.len())
        };
        let app = cx.entity();
        let picker = cx.new(|_cx| CompactLevelPicker::new(app, heading));
        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme().clone();
            dialog
                .title("Compact level")
                .overlay_closable(true)
                .keyboard(true)
                .w(px(660.))
                .border_color(theme.border.opacity(0.32))
                .child(picker.clone())
        });
    }
}
