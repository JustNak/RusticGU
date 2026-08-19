//! Steam library cards + compact actions.

use gpui::{
    div, prelude::FluentBuilder, px, Context, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex, v_flex, ActiveTheme, Disableable, Icon, IconName, Sizable, StyledExt,
};

use super::widgets::{empty_state_badge, styled_progress};
use super::LibraryApp;
use crate::compact::CompactOp;
use crate::format::format_size_pair;
use crate::library::SteamGame;

impl LibraryApp {
    pub(crate) fn render_library(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let games = self.visible_games(cx);
        let selected = self.selected_app_id;
        let scanning = self.library_scanning;
        let error = self.library_error.clone();
        let progress = self.compact_progress.clone();
        let busy = self.compact_busy;

        v_flex()
            .id("library-view")
            .size_full()
            .bg(theme.background)
            .p_5()
            .gap_4()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_lg()
                                    .font_bold()
                                    .text_color(theme.foreground)
                                    .child("Steam library"),
                            )
                            .child(div().text_xs().text_color(theme.muted_foreground).child(
                                if scanning {
                                    "Scanning Steam libraries…".to_string()
                                } else {
                                    format!("{} games", self.games.len())
                                },
                            )),
                    )
                    .child(
                        Button::new("library-rescan")
                            .outline()
                            .label("Rescan")
                            .icon(Icon::empty().path("icons/rotate-cw.svg"))
                            .on_click(cx.listener(|this, _, _, cx| this.refresh_library(cx))),
                    ),
            )
            .when_some(error, |el, msg| {
                el.child(
                    div()
                        .p_3()
                        .rounded(theme.radius)
                        .border_1()
                        .border_color(theme.danger.opacity(0.4))
                        .text_sm()
                        .text_color(theme.danger)
                        .child(msg),
                )
            })
            .when_some(progress, |el, p| {
                el.child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(p.message),
                        )
                        .child(styled_progress(
                            if p.total == 0 {
                                0.0
                            } else {
                                (p.processed as f32 / p.total as f32) * 100.0
                            },
                            theme.progress_bar,
                            self.settings.progress_style,
                        )),
                )
            })
            .child(if games.is_empty() && !scanning {
                self.render_empty_library(cx).into_any_element()
            } else {
                div()
                    .id("library-cards-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap_3()
                            .children(games.into_iter().map(|game| {
                                let is_selected = selected == Some(game.app_id);
                                render_game_card(game, is_selected, busy, cx)
                            })),
                    )
                    .into_any_element()
            })
    }

    fn render_empty_library(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .child(empty_state_badge(
                IconName::Inbox,
                theme.primary,
                theme.secondary,
                theme.border,
                self.settings.reduce_motion,
            ))
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("No Steam games found. Install Steam or rescan."),
            )
    }
}

fn render_game_card(
    game: SteamGame,
    selected: bool,
    busy: bool,
    cx: &mut Context<LibraryApp>,
) -> impl IntoElement {
    let theme = cx.theme().clone();
    let app_id = game.app_id;
    let size = format_size_pair(game.logical_bytes, game.on_disk_bytes);
    let compacted = game
        .on_disk_bytes
        .zip(game.logical_bytes)
        .is_some_and(|(disk, logical)| disk < logical.saturating_sub(logical / 20));
    let path = game.install_path.display().to_string();

    v_flex()
        .id(SharedString::from(format!("game-card-{app_id}")))
        .w(px(260.))
        .min_h(px(168.))
        .p_3()
        .gap_2()
        .rounded(theme.radius_lg)
        .border_1()
        .border_color(if selected {
            theme.list_active_border
        } else {
            theme.border.opacity(0.55)
        })
        .bg(if selected {
            theme.list_active
        } else {
            theme
                .secondary
                .opacity(if theme.is_dark() { 0.35 } else { 0.55 })
        })
        .hover(|s| s.bg(theme.secondary.opacity(0.7)))
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, _, cx| {
            this.select_game(app_id, cx);
        }))
        .child(
            h_flex()
                .w_full()
                .items_start()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .font_semibold()
                        .text_color(theme.foreground)
                        .child(game.name.clone()),
                )
                .child(
                    div()
                        .px_1p5()
                        .py_0p5()
                        .rounded(theme.radius)
                        .bg(theme.primary.opacity(0.16))
                        .text_xs()
                        .text_color(theme.primary)
                        .child(if compacted { "Compacted" } else { "Inflated" }),
                ),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(format!("Steam {app_id}")),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(size),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground.opacity(0.8))
                .child(path),
        )
        .when(selected, |el| {
            el.child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new(SharedString::from(format!("estimate-{app_id}")))
                            .outline()
                            .small()
                            .label("Estimate")
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.estimate_selected(window, cx);
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("compact-{app_id}")))
                            .primary()
                            .small()
                            .label("Compact")
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.start_compact(CompactOp::Compress, window, cx);
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("undo-{app_id}")))
                            .outline()
                            .small()
                            .label("Undo")
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.start_compact(CompactOp::Uncompress, window, cx);
                            })),
                    ),
            )
        })
}
