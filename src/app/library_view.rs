//! Cover-art gallery + overlay compact actions.

use gpui::{
    div, img, prelude::FluentBuilder, px, ClickEvent, Context, InteractiveElement, IntoElement,
    ObjectFit, ParentElement, SharedString, StatefulInteractiveElement, Styled, StyledImage,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    tooltip::Tooltip,
    v_flex, ActiveTheme, Disableable, Icon, Sizable, StyledExt,
};

use super::widgets::styled_progress;
use super::FilterKind;
use super::LibraryApp;
use crate::covers::Monogram;
use crate::format::format_size_pair;
use crate::library::{title_is_compact_excluded, LibraryTitle};
use crate::settings::UiDensity;

const POSTER_RADIUS: f32 = 10.0;
const POSTER_GAP: f32 = 8.0;
const UNSELECTED_DIM: f32 = 0.6;

impl LibraryApp {
    pub(crate) fn render_library(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let games = self.visible_games(cx);
        let selected = self.selected_ids.clone();
        let scanning = self.library_scanning;
        let error = self.library_error.clone();
        let progress = self.compact_progress.clone();
        let busy = self.compact_busy;
        let hovered = self.hovered_id.clone();
        let filter = self.filter;
        let selected_n = self.selected_titles().len();

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
                    .gap_3()
                    .child(
                        div()
                            .text_lg()
                            .font_bold()
                            .text_color(theme.foreground)
                            .child(match filter {
                                FilterKind::Compacted => "Compacted",
                                FilterKind::Uncompacted => "Uncompacted",
                                _ => "Library",
                            }),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(div().text_xs().text_color(theme.muted_foreground).child(
                                if scanning {
                                    "Scanning launchers…".to_string()
                                } else if selected_n > 1 {
                                    format!("{selected_n} selected · {} games", self.games.len())
                                } else {
                                    format!("{} games", self.games.len())
                                },
                            ))
                            .when(selected_n > 0, |el| {
                                el.child(
                                    Button::new("library-compact-selected")
                                        .primary()
                                        .small()
                                        .icon(Icon::empty().path("icons/file-archive.svg"))
                                        .label(if selected_n == 1 {
                                            "Compress".into()
                                        } else {
                                            format!("Compress {selected_n}")
                                        })
                                        .disabled(busy)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.open_compact_level_dialog(window, cx);
                                        })),
                                )
                            })
                            .child(
                                Button::new("library-refresh")
                                    .outline()
                                    .label("Refresh")
                                    .icon(Icon::empty().path("icons/rotate-cw.svg"))
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.refresh_library(cx)),
                                    ),
                            ),
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
                let (poster_w, poster_h) = poster_size(self.settings.ui_density);
                div()
                    .id("library-gallery-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap(px(POSTER_GAP))
                            .children(games.into_iter().map(|game| {
                                let id = game.id.clone();
                                let is_selected = selected.contains(&id);
                                let is_hovered = hovered.as_deref() == Some(id.as_str());
                                let cover = self.cover_image(&id);
                                render_poster_card(
                                    game,
                                    cover,
                                    PosterChrome {
                                        selected: is_selected,
                                        hovered: is_hovered,
                                        busy,
                                        width: poster_w,
                                        height: poster_h,
                                    },
                                    cx,
                                )
                            })),
                    )
                    .into_any_element()
            })
    }

    fn render_empty_library(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let query = self.search_input.read(cx).value();
        let filtered = !query.trim().is_empty() || self.filter != FilterKind::Library;
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(if filtered {
                        "No titles match this filter."
                    } else {
                        "No games found."
                    }),
            )
            .child(
                Button::new("library-empty-refresh")
                    .outline()
                    .label("Refresh")
                    .icon(Icon::empty().path("icons/rotate-cw.svg"))
                    .on_click(cx.listener(|this, _, _, cx| this.refresh_library(cx))),
            )
    }
}

fn poster_size(density: UiDensity) -> (f32, f32) {
    match density {
        UiDensity::Comfortable => (200.0, 300.0),
        UiDensity::Compact => (180.0, 270.0),
    }
}

fn card_dom_id(id: &str) -> SharedString {
    let safe: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!("game-poster-{safe}").into()
}

struct PosterChrome {
    selected: bool,
    hovered: bool,
    busy: bool,
    width: f32,
    height: f32,
}

fn render_poster_card(
    game: LibraryTitle,
    cover: Option<std::sync::Arc<gpui::RenderImage>>,
    chrome: PosterChrome,
    cx: &mut Context<LibraryApp>,
) -> impl IntoElement {
    let theme = cx.theme().clone();
    let id = game.id.clone();
    let size = format_size_pair(game.logical_bytes, game.on_disk_bytes);
    let path = game.install_path.display().to_string();
    let tip = format!("{} — {size} — {path}", game.name);
    let has_art = cover.is_some();
    let monogram = Monogram::from_title(&game);
    let badge = game.store.badge();
    let excluded = title_is_compact_excluded(&game);
    let poster_w = chrome.width;
    let poster_h = chrome.height;
    let selected = chrome.selected;
    let hovered = chrome.hovered;
    let busy = chrome.busy;
    let dim = if selected || hovered {
        1.0
    } else {
        UNSELECTED_DIM
    };

    v_flex()
        .id(card_dom_id(&id))
        .w(px(poster_w))
        .opacity(dim)
        .on_hover({
            let id = id.clone();
            cx.listener(move |this, hovering: &bool, _, cx| {
                if *hovering {
                    this.hovered_id = Some(id.clone());
                } else if this.hovered_id.as_deref() == Some(id.as_str()) {
                    this.hovered_id = None;
                }
                cx.notify();
            })
        })
        .child(
            div()
                .id(SharedString::from(format!("poster-art-{id}")))
                .relative()
                .w(px(poster_w))
                .h(px(poster_h))
                .rounded(px(POSTER_RADIUS))
                .overflow_hidden()
                .border_1()
                .border_color(if selected {
                    theme.primary
                } else {
                    theme.border.opacity(0.28)
                })
                .bg(theme.secondary)
                .cursor_pointer()
                .tooltip({
                    let tip = SharedString::from(tip);
                    move |window, cx| Tooltip::new(tip.clone()).build(window, cx)
                })
                .on_click({
                    let id = id.clone();
                    cx.listener(move |this, ev: &ClickEvent, _, cx| {
                        let multi = ev.modifiers().secondary();
                        this.select_game_click(id.clone(), multi, cx);
                    })
                })
                .child(if has_art {
                    div().size_full().into_any_element()
                } else {
                    render_monogram_tile(&monogram, badge, theme.muted_foreground, theme.foreground)
                        .into_any_element()
                })
                .when_some(cover, |el, image| {
                    el.child(
                        img(image)
                            .absolute()
                            .inset_0()
                            .size_full()
                            .object_fit(ObjectFit::Cover),
                    )
                })
                .when(hovered, |el| {
                    el.child(render_play_veil(&id, busy || excluded, cx))
                }),
        )
}

fn render_monogram_tile(
    monogram: &Monogram,
    badge: &'static str,
    muted: gpui::Hsla,
    fg: gpui::Hsla,
) -> impl IntoElement {
    v_flex()
        .id(SharedString::from(format!("monogram-{}", monogram.title)))
        .size_full()
        .items_center()
        .justify_center()
        .gap_2()
        .p_3()
        .child(
            div()
                .text_3xl()
                .font_bold()
                .text_color(muted)
                .child(monogram.initials.clone()),
        )
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(fg)
                .child(monogram.title.clone()),
        )
        .child(
            div()
                .px_1p5()
                .py_0p5()
                .rounded(px(4.))
                .text_xs()
                .text_color(muted)
                .child(badge),
        )
}

fn render_play_veil(id: &str, disabled: bool, cx: &mut Context<LibraryApp>) -> impl IntoElement {
    let id = id.to_string();
    v_flex()
        .id(SharedString::from(format!("poster-veil-{id}")))
        .absolute()
        .inset_0()
        .items_center()
        .justify_center()
        .bg(gpui::hsla(0.0, 0.0, 0.0, 0.46))
        .occlude()
        .child(
            Button::new(SharedString::from(format!("poster-launch-{id}")))
                .primary()
                .small()
                .icon(Icon::empty().path("icons/play.svg"))
                .label("Play")
                .disabled(disabled)
                .on_click({
                    let id = id.clone();
                    cx.listener(move |this, _, window, cx| {
                        this.select_game(id.clone(), cx);
                        this.launch_selected(window, cx);
                    })
                }),
        )
}

#[cfg(test)]
mod tests {
    use super::{poster_size, POSTER_GAP, POSTER_RADIUS, UNSELECTED_DIM};
    use crate::settings::UiDensity;

    #[test]
    fn posters_are_portrait_2_by_3_in_spec() {
        for density in [UiDensity::Comfortable, UiDensity::Compact] {
            let (w, h) = poster_size(density);
            assert!((180.0..=220.0).contains(&w), "width {w}");
            assert!((w * 3.0 - h * 2.0).abs() < 0.01, "2:3 {w}x{h}");
        }
        assert!((8.0..=12.0).contains(&POSTER_RADIUS));
        assert!((6.0..=10.0).contains(&POSTER_GAP));
        assert!((UNSELECTED_DIM - 0.6).abs() < 0.001);
    }
}
