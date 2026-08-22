//! Cover-art gallery + overlay compact actions.

use std::time::Duration;

use super::compact_apply::TitleActivity;
use super::inspector::inspector_width;
use super::widgets::styled_progress;
use super::FilterKind;
use super::LibraryApp;
use crate::appearance::title_tint;
use crate::branding::{APP_LOGO_DARK, APP_LOGO_LIGHT};
use crate::covers::Monogram;
use crate::format::format_bytes;
use crate::library::{LibraryStore, LibraryTitle};
use crate::settings::UiDensity;
use gpui::{
    div, hsla, img, prelude::FluentBuilder, pulsating_between, px, Animation, AnimationExt,
    ClickEvent, Context, Hsla, InteractiveElement, IntoElement, ObjectFit, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, StyledImage,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    tooltip::Tooltip,
    v_flex, ActiveTheme, Disableable, Icon, Sizable, StyledExt,
};

const POSTER_RADIUS: f32 = 12.0;
const POSTER_GAP: f32 = 10.0;
const UNSELECTED_DIM: f32 = 0.84;

impl LibraryApp {
    pub(crate) fn render_library(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let games = self.visible_games(cx);
        let selected = self.selected_ids.clone();
        let scanning = self.library_scanning;
        let error = self.library_error.clone();
        let poster_job = self.poster_job.clone();
        let progress_style = self.settings.progress_style;
        let busy = self.compact_busy || self.compact_flow.is_some();
        let filter = self.filter;
        let selected_n = self.selected_titles().len();
        let shown = games.len();
        let total = self.games.len();
        let live_paused = self.live.paused();
        let live = self.live.clone();
        let heading = match filter {
            FilterKind::Compacted => "Compacted",
            FilterKind::Uncompacted => "Uncompacted",
            FilterKind::Store(store) => store.badge(),
            _ => "Library",
        };
        let count_label = if scanning {
            "Scanning launchers…".to_string()
        } else if selected_n > 1 {
            format!("{selected_n} selected · {shown} shown")
        } else if shown != total {
            format!("{shown} of {total}")
        } else {
            format!("{total} games")
        };

        let header = h_flex()
            .id("library-header")
            .w_full()
            .flex_shrink_0()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(8.)).h(px(8.)).rounded_full().bg(theme.primary))
                    .child(
                        div()
                            .text_lg()
                            .font_bold()
                            .text_color(theme.foreground)
                            .child(heading),
                    )
                    .child(render_live_chip(live_paused, &theme, cx)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(count_label),
                    )
                    .child(
                        Button::new("library-add-folder")
                            .outline()
                            .small()
                            .label("Add folder")
                            .icon(Icon::empty().path("icons/plus.svg"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.prompt_add_custom_game_directory(window, cx);
                            })),
                    ),
            );

        let main = if games.is_empty() && scanning {
            self.render_scanning_library(cx).into_any_element()
        } else if games.is_empty() {
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
                        .id("library-gallery")
                        .w_full()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .gap(px(POSTER_GAP))
                        .children(games.into_iter().map(|game| {
                            let id = game.id.clone();
                            let is_selected = selected.contains(&id);
                            let cover = self.cover_image(&id);
                            let activity =
                                TitleActivity::resolve(&game, poster_job.as_ref(), &live);
                            render_poster_card(
                                game,
                                cover,
                                PosterChrome {
                                    selected: is_selected,
                                    busy,
                                    width: poster_w,
                                    height: poster_h,
                                    activity,
                                    progress_style,
                                },
                                cx,
                            )
                        })),
                )
                .into_any_element()
        };

        let inspector_w = inspector_width(self.settings.ui_density);
        v_flex()
            .id("library-view")
            .relative()
            .size_full()
            .bg(theme.background)
            .p_5()
            .when(shown > 0, |el| el.pr(px(inspector_w + 20.0)))
            .gap_4()
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
            .child(header)
            .child(main)
            .when(shown > 0, |el| {
                el.child(
                    div()
                        .id("library-inspector-slot")
                        .absolute()
                        .top_0()
                        .right_0()
                        .bottom_0()
                        .w(px(inspector_w))
                        .child(self.render_inspector(cx)),
                )
            })
    }

    fn render_scanning_library(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        v_flex()
            .id("library-scanning")
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .child(
                div()
                    .text_lg()
                    .font_bold()
                    .text_color(theme.foreground)
                    .child("Scanning launchers…"),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("Steam, Epic, GOG, and anything you added as a folder."),
            )
    }

    fn render_empty_library(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let query = self.search_input.read(cx).value();
        let filtered = !query.trim().is_empty() || !self.filter.shows_all_library();
        let logo = if theme.is_dark() {
            APP_LOGO_DARK
        } else {
            APP_LOGO_LIGHT
        };
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                img(logo)
                    .w(px(72.))
                    .h(px(72.))
                    .rounded(px(16.))
                    .object_fit(ObjectFit::Cover),
            )
            .child(
                div()
                    .text_lg()
                    .font_bold()
                    .text_color(theme.foreground)
                    .child(if filtered {
                        "Nothing matches"
                    } else {
                        "Your library is empty"
                    }),
            )
            .child(
                div()
                    .max_w(px(360.))
                    .text_sm()
                    .text_center()
                    .text_color(theme.muted_foreground)
                    .child(if filtered {
                        "Try another filter or clear the search."
                    } else {
                        "Scan Steam and other launchers, or add a game folder to start compacting."
                    }),
            )
            .when(!filtered, |el| {
                el.child(
                    Button::new("library-empty-add-folder")
                        .primary()
                        .label("Add folder")
                        .icon(Icon::empty().path("icons/plus.svg"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.prompt_add_custom_game_directory(window, cx);
                        })),
                )
            })
            .child(
                Button::new("library-empty-refresh")
                    .when(filtered, |b| b.primary())
                    .when(!filtered, |b| b.outline())
                    .label("Refresh")
                    .icon(Icon::empty().path("icons/rotate-cw.svg"))
                    .on_click(cx.listener(|this, _, _, cx| this.refresh_library(cx))),
            )
    }
}

fn render_live_chip(
    paused: bool,
    theme: &gpui_component::Theme,
    cx: &mut Context<LibraryApp>,
) -> impl IntoElement {
    let color = if paused {
        theme.muted_foreground
    } else {
        theme.success
    };
    h_flex()
        .id("library-live-chip")
        .h(px(26.))
        .items_center()
        .gap_1p5()
        .px_2()
        .rounded(theme.radius)
        .border_1()
        .border_color(if paused {
            theme.border
        } else {
            theme.success.opacity(0.45)
        })
        .cursor_pointer()
        .hover(|s| s.bg(theme.secondary.opacity(0.55)))
        .on_click(cx.listener(|this, _, _, cx| {
            this.toggle_live_compact(cx);
        }))
        .child(div().w(px(7.)).h(px(7.)).rounded_full().bg(color))
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(color)
                .child(if paused {
                    "Live paused"
                } else {
                    "Live Compact on"
                }),
        )
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
    busy: bool,
    width: f32,
    height: f32,
    activity: TitleActivity,
    progress_style: crate::settings::ProgressStyle,
}

fn render_poster_card(
    game: LibraryTitle,
    cover: Option<std::sync::Arc<gpui::RenderImage>>,
    chrome: PosterChrome,
    cx: &mut Context<LibraryApp>,
) -> impl IntoElement {
    let theme = cx.theme().clone();
    let id = game.id.clone();
    let has_art = cover.is_some();
    let monogram = Monogram::from_title(&game);
    let store_icon = game.store.icon_path();
    let store_name = game.store.badge();
    let compacted = game.is_compacted();
    let activity = chrome.activity;
    let hover_spec = poster_hover_spec(&activity, compacted);
    let poster_w = chrome.width;
    let poster_h = chrome.height;
    let selected = chrome.selected;
    let busy = chrome.busy;
    let progress_style = chrome.progress_style;
    let group = card_dom_id(&id);
    let show_status = matches!(
        activity,
        TitleActivity::Job { .. } | TitleActivity::Patching
    );
    let name = game.name.clone();
    let caption = caption_size(&game);
    let tint = title_tint(&name, theme.is_dark());

    v_flex()
        .id(group.clone())
        .group(group.clone())
        .w(px(poster_w))
        .gap_1p5()
        .opacity(if selected { 1.0 } else { UNSELECTED_DIM })
        .when(!selected, |el| el.hover(|s| s.opacity(1.0)))
        .child(
            div()
                .id(SharedString::from(format!("poster-art-{id}")))
                .relative()
                .w(px(poster_w))
                .h(px(poster_h))
                .rounded(px(POSTER_RADIUS))
                .overflow_hidden()
                .bg(if has_art { theme.secondary } else { tint })
                .cursor_pointer()
                .on_click({
                    let id = id.clone();
                    cx.listener(move |this, ev: &ClickEvent, _, cx| {
                        let multi = ev.modifiers().secondary();
                        this.select_game_click(id.clone(), multi, cx);
                    })
                })
                .child(if let Some(image) = cover {
                    img(image)
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .into_any_element()
                } else {
                    render_monogram_tile(&monogram, theme.foreground).into_any_element()
                })
                .child(render_poster_frame_ring(selected, group.clone(), &theme))
                .when_some(store_icon, |el, path| {
                    el.child(render_store_badge(&id, path, store_name))
                })
                .when(
                    poster_shows_compacted_badge(compacted, &activity) && !show_status,
                    |el| el.child(render_compacted_badge(&id, &theme)),
                )
                .when(show_status, |el| {
                    el.child(render_poster_status_overlay(
                        &id,
                        &activity,
                        &theme,
                        progress_style,
                    ))
                })
                .when(!show_status, |el| {
                    el.child(render_poster_veil(
                        &id, group, hover_spec, game.store, busy, cx,
                    ))
                }),
        )
        .child(
            v_flex()
                .id(SharedString::from(format!("poster-caption-{id}")))
                .w_full()
                .gap_0p5()
                .px_0p5()
                .cursor_pointer()
                .on_click({
                    let id = id.clone();
                    cx.listener(move |this, ev: &ClickEvent, _, cx| {
                        let multi = ev.modifiers().secondary();
                        this.select_game_click(id.clone(), multi, cx);
                    })
                })
                .child(
                    div()
                        .w_full()
                        .text_sm()
                        .font_semibold()
                        .text_color(theme.foreground)
                        .truncate()
                        .child(name),
                )
                .when(!caption.is_empty(), |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .truncate()
                            .child(caption),
                    )
                }),
        )
}

fn caption_size(game: &LibraryTitle) -> String {
    match (game.on_disk_bytes, game.logical_bytes) {
        (Some(disk), Some(logical)) if disk + logical / 20 < logical => format_bytes(disk),
        (Some(disk), _) => format_bytes(disk),
        (None, Some(logical)) => format_bytes(logical),
        _ => String::new(),
    }
}

/// Hairline drawn on top of the cover. Putting the border on the clip frame
/// shrinks the image and leaves a gap in the rounded corners.
fn render_poster_frame_ring(
    selected: bool,
    group: SharedString,
    theme: &gpui_component::Theme,
) -> impl IntoElement {
    div()
        .absolute()
        .inset_0()
        .rounded(px(POSTER_RADIUS))
        .border_2()
        .border_color(if selected {
            theme.primary
        } else {
            theme.border.opacity(0.22)
        })
        .group_hover(group, |s| s.border_color(theme.primary.opacity(0.55)))
}

fn render_store_badge(id: &str, icon_path: &'static str, name: &'static str) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("poster-store-{id}")))
        .absolute()
        .top(px(8.))
        .right(px(8.))
        .tooltip({
            let tip = SharedString::from(name);
            move |window, cx| Tooltip::new(tip.clone()).build(window, cx)
        })
        .child(
            Icon::empty()
                .path(icon_path)
                .with_size(px(18.))
                .text_color(hsla(0.0, 0.0, 1.0, 0.95)),
        )
}

fn render_compacted_badge(id: &str, theme: &gpui_component::Theme) -> impl IntoElement {
    h_flex()
        .id(SharedString::from(format!("poster-compacted-{id}")))
        .absolute()
        .top(px(8.))
        .left(px(8.))
        .items_center()
        .gap_1()
        .px_1p5()
        .py_0p5()
        .rounded(px(6.))
        .bg(theme.primary.opacity(0.90))
        .child(
            Icon::empty()
                .path("icons/file-archive.svg")
                .with_size(px(11.))
                .text_color(theme.primary_foreground),
        )
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(theme.primary_foreground)
                .child("Compacted"),
        )
}

fn render_monogram_tile(monogram: &Monogram, fg: Hsla) -> impl IntoElement {
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
                .text_color(fg)
                .child(monogram.initials.clone()),
        )
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(fg)
                .opacity(0.92)
                .child(monogram.title.clone()),
        )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PosterHoverSpec {
    show_compress: bool,
    show_decompress: bool,
    show_change_method: bool,
    show_play: bool,
    show_excluded_hint: bool,
}

fn poster_hover_spec(activity: &TitleActivity, compacted: bool) -> PosterHoverSpec {
    match activity {
        TitleActivity::Excluded => PosterHoverSpec {
            show_compress: false,
            show_decompress: false,
            show_change_method: false,
            show_play: true,
            show_excluded_hint: true,
        },
        TitleActivity::Patching | TitleActivity::Job { .. } => PosterHoverSpec {
            show_compress: false,
            show_decompress: false,
            show_change_method: false,
            show_play: false,
            show_excluded_hint: false,
        },
        TitleActivity::Idle if compacted => PosterHoverSpec {
            show_compress: false,
            show_decompress: true,
            show_change_method: true,
            show_play: true,
            show_excluded_hint: false,
        },
        TitleActivity::Idle => PosterHoverSpec {
            show_compress: true,
            show_decompress: false,
            show_change_method: false,
            show_play: true,
            show_excluded_hint: false,
        },
    }
}

fn poster_shows_compacted_badge(compacted: bool, activity: &TitleActivity) -> bool {
    compacted && matches!(activity, TitleActivity::Idle)
}

fn render_poster_status_overlay(
    id: &str,
    activity: &TitleActivity,
    theme: &gpui_component::Theme,
    style: crate::settings::ProgressStyle,
) -> impl IntoElement {
    let label = activity.heading().unwrap_or("");
    let message = activity.detail().unwrap_or_default();
    let pct = activity.percent();
    v_flex()
        .id(SharedString::from(format!("poster-job-{id}")))
        .absolute()
        .inset_0()
        .justify_end()
        .p_2()
        .gap_1p5()
        .bg(hsla(0.62, 0.04, 0.04, 0.72))
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(theme.foreground)
                .child(label),
        )
        .child(
            div()
                .w_full()
                .text_xs()
                .text_color(theme.muted_foreground)
                .truncate()
                .child(message),
        )
        .when_some(pct, |el, pct| {
            el.child(
                div()
                    .w_full()
                    .child(styled_progress(pct, theme.progress_bar, style))
                    .with_animation(
                        SharedString::from(format!("poster-job-pulse-{id}")),
                        Animation::new(Duration::from_secs(2))
                            .repeat()
                            .with_easing(pulsating_between(0.72, 1.0)),
                        |this, delta| this.opacity(delta),
                    ),
            )
        })
}

fn render_poster_veil(
    id: &str,
    group: SharedString,
    spec: PosterHoverSpec,
    store: LibraryStore,
    busy: bool,
    cx: &mut Context<LibraryApp>,
) -> impl IntoElement {
    let theme = cx.theme().clone();
    let id = id.to_string();
    let play_label = store.launch_label();
    // Stay mounted and do not occlude: inserting an occluding veil on hover
    // steals the card hitbox, which drops hover and flickers the actions.
    v_flex()
        .id(SharedString::from(format!("poster-veil-{id}")))
        .absolute()
        .inset_0()
        .justify_end()
        .p_2()
        .gap_1p5()
        .bg(hsla(0.62, 0.04, 0.04, 0.52))
        .invisible()
        .group_hover(group, |s| s.visible())
        .when(spec.show_excluded_hint, |el| {
            el.child(
                div()
                    .w_full()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("Excluded from compact"),
            )
        })
        .when(spec.show_compress, |el| {
            el.child(
                Button::new(SharedString::from(format!("poster-compress-{id}")))
                    .primary()
                    .small()
                    .compact()
                    .w_full()
                    .icon(Icon::empty().path("icons/file-archive.svg"))
                    .label("Compress")
                    .tooltip("Compress this title")
                    .disabled(busy)
                    .on_click({
                        let id = id.clone();
                        cx.listener(move |this, _, window, cx| {
                            this.begin_title_compress(id.clone(), window, cx);
                        })
                    }),
            )
        })
        .when(spec.show_decompress, |el| {
            el.child(
                Button::new(SharedString::from(format!("poster-decompress-{id}")))
                    .outline()
                    .small()
                    .compact()
                    .w_full()
                    .icon(Icon::empty().path("icons/undo-2.svg"))
                    .label("Decompress")
                    .tooltip("Restore uncompressed files")
                    .disabled(busy)
                    .on_click({
                        let id = id.clone();
                        cx.listener(move |this, _, window, cx| {
                            this.begin_title_decompress(id.clone(), window, cx);
                        })
                    }),
            )
        })
        .when(spec.show_change_method, |el| {
            el.child(
                Button::new(SharedString::from(format!("poster-change-{id}")))
                    .primary()
                    .small()
                    .compact()
                    .w_full()
                    .icon(Icon::empty().path("icons/redo-2.svg"))
                    .label("Change")
                    .tooltip("Change compression method")
                    .disabled(busy)
                    .on_click({
                        let id = id.clone();
                        cx.listener(move |this, _, window, cx| {
                            this.begin_title_change_method(id.clone(), window, cx);
                        })
                    }),
            )
        })
        .when(spec.show_play, |el| {
            el.child(
                Button::new(SharedString::from(format!("poster-launch-{id}")))
                    .outline()
                    .small()
                    .compact()
                    .w_full()
                    .icon(Icon::empty().path(store.launch_icon_path()))
                    .label(play_label)
                    .tooltip(play_label)
                    .disabled(busy)
                    .on_click({
                        let id = id.clone();
                        cx.listener(move |this, _, window, cx| {
                            this.select_game(id.clone(), cx);
                            this.launch_selected(window, cx);
                        })
                    }),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::super::compact_apply::{PosterJob, PosterJobKind};
    use super::TitleActivity;
    use super::{
        poster_hover_spec, poster_shows_compacted_badge, poster_size, POSTER_GAP, POSTER_RADIUS,
        UNSELECTED_DIM,
    };
    use crate::compact::CompactProgress;
    use crate::library::LibraryStore;
    use crate::live::LiveHandle;
    use crate::settings::UiDensity;
    use stores::StoreId;

    #[test]
    fn posters_are_portrait_2_by_3_in_spec() {
        for density in [UiDensity::Comfortable, UiDensity::Compact] {
            let (w, h) = poster_size(density);
            assert!((180.0..=220.0).contains(&w), "width {w}");
            assert!((w * 3.0 - h * 2.0).abs() < 0.01, "2:3 {w}x{h}");
        }
        assert!((8.0..=12.0).contains(&POSTER_RADIUS));
        assert!((6.0..=12.0).contains(&POSTER_GAP));
        assert!((UNSELECTED_DIM - 0.84).abs() < 0.001);
    }

    #[test]
    fn store_badge_is_an_icon_for_known_launchers() {
        assert_eq!(
            LibraryStore::Steam.icon_path(),
            Some("icons/store-steam.svg")
        );
        assert_eq!(
            LibraryStore::Extra(StoreId::Epic).icon_path(),
            Some("icons/store-epic.svg")
        );
        assert_eq!(
            LibraryStore::Extra(StoreId::XboxGames).icon_path(),
            Some("icons/store-xbox.svg")
        );
        assert_eq!(LibraryStore::Custom.icon_path(), Some("icons/folder.svg"));
    }

    #[test]
    fn uncompacted_hover_leads_with_compress() {
        let spec = poster_hover_spec(&TitleActivity::Idle, false);
        assert!(spec.show_compress);
        assert!(spec.show_play);
        assert!(!spec.show_decompress);
        assert!(!spec.show_change_method);
        assert!(!spec.show_excluded_hint);
        assert!(!poster_shows_compacted_badge(false, &TitleActivity::Idle));
    }

    #[test]
    fn compacted_hover_offers_decompress_and_change() {
        let spec = poster_hover_spec(&TitleActivity::Idle, true);
        assert!(!spec.show_compress);
        assert!(spec.show_decompress);
        assert!(spec.show_change_method);
        assert!(spec.show_play);
        assert!(poster_shows_compacted_badge(true, &TitleActivity::Idle));
        assert!(!poster_shows_compacted_badge(
            true,
            &TitleActivity::Excluded
        ));
        assert!(!poster_shows_compacted_badge(
            true,
            &TitleActivity::Patching
        ));
    }

    #[test]
    fn excluded_hover_is_play_only() {
        let spec = poster_hover_spec(&TitleActivity::Excluded, true);
        assert!(!spec.show_compress);
        assert!(!spec.show_decompress);
        assert!(!spec.show_change_method);
        assert!(spec.show_play);
        assert!(spec.show_excluded_hint);
    }

    #[test]
    fn patching_hides_compact_hover_actions() {
        let spec = poster_hover_spec(&TitleActivity::Patching, true);
        assert!(!spec.show_compress);
        assert!(!spec.show_decompress);
        assert!(!spec.show_change_method);
        assert!(!spec.show_play);
    }

    #[test]
    fn hover_play_copy_follows_the_store() {
        assert_eq!(LibraryStore::Steam.launch_label(), "Play");
        assert_eq!(LibraryStore::Custom.launch_label(), "Open folder");
        assert_eq!(LibraryStore::Steam.launch_icon_path(), "icons/play.svg");
        assert_eq!(
            LibraryStore::Custom.launch_icon_path(),
            "icons/folder-open.svg"
        );
    }

    #[test]
    fn poster_job_overlay_uses_file_progress_and_waiting() {
        let live = LiveHandle::for_tests();
        let job = PosterJob {
            title_ids: vec!["steam:1".into(), "steam:2".into()],
            current_id: "steam:1".into(),
            kind: PosterJobKind::Change,
            progress: CompactProgress {
                processed: 40,
                total: 80,
                message: "WOF /EXE 40/80…".into(),
            },
        };
        let current_title = crate::library::LibraryTitle {
            id: "steam:1".into(),
            name: "A".into(),
            install_path: std::path::PathBuf::from(r"D:\A"),
            store: LibraryStore::Steam,
            launcher_id: None,
            last_played_unix: None,
            logical_bytes: None,
            on_disk_bytes: None,
            compacted: false,
            steam_app_id: Some(1),
            steam_library_path: None,
            steam_install_dir_name: None,
            cover_url: None,
        };
        let queued_title = crate::library::LibraryTitle {
            id: "steam:2".into(),
            ..current_title.clone()
        };
        let current = TitleActivity::resolve(&current_title, Some(&job), &live);
        assert_eq!(current.heading(), Some("Changing"));
        assert!((current.percent().unwrap() - 50.0).abs() < 0.01);
        let queued = TitleActivity::resolve(&queued_title, Some(&job), &live);
        assert_eq!(queued.heading(), Some("Waiting…"));
        assert_eq!(queued.percent(), Some(0.0));
    }
}
