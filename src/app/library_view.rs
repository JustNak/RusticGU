//! Cover-art gallery + overlay compact actions.

use std::time::Duration;

use super::compact_apply::{PosterJob, PosterJobKind};
use super::widgets::styled_progress;
use super::FilterKind;
use super::LibraryApp;
use crate::appearance::title_tint;
use crate::branding::{APP_LOGO_DARK, APP_LOGO_LIGHT};
use crate::compact::CompactProgress;
use crate::covers::Monogram;
use crate::format::{format_bytes, format_size_pair};
use crate::library::{title_is_compact_excluded, LibraryTitle};
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
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(div().w(px(8.)).h(px(8.)).rounded_full().bg(theme.primary))
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
                            ),
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
                            .when_some(header_compact_label(selected_n), |el, label| {
                                el.child(
                                    Button::new("library-compact-selected")
                                        .primary()
                                        .small()
                                        .icon(Icon::empty().path("icons/file-archive.svg"))
                                        .label(label)
                                        .disabled(busy)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.open_compact_flow(window, cx);
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
                                let cover = self.cover_image(&id);
                                render_poster_card(
                                    game,
                                    cover,
                                    PosterChrome {
                                        selected: is_selected,
                                        busy,
                                        width: poster_w,
                                        height: poster_h,
                                        job: poster_job_view(poster_job.as_ref(), &id),
                                        progress_style,
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
                        "Scan Steam and other launchers to start compacting games."
                    }),
            )
            .child(
                Button::new("library-empty-refresh")
                    .primary()
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
    busy: bool,
    width: f32,
    height: f32,
    job: Option<PosterJobView>,
    progress_style: crate::settings::ProgressStyle,
}

#[derive(Debug, Clone)]
struct PosterJobView {
    kind: PosterJobKind,
    waiting: bool,
    progress: CompactProgress,
}

fn poster_job_view(job: Option<&PosterJob>, id: &str) -> Option<PosterJobView> {
    let job = job?;
    if !job.covers(id) {
        return None;
    }
    Some(PosterJobView {
        kind: job.kind,
        waiting: job.waiting(id),
        progress: job.progress.clone(),
    })
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
    let tip = format!("{}: {size} ({path})", game.name);
    let has_art = cover.is_some();
    let monogram = Monogram::from_title(&game);
    let store_icon = game.store.icon_path();
    let store_name = game.store.badge();
    let excluded = title_is_compact_excluded(&game);
    let compacted = game.is_compacted();
    let hover_spec = poster_hover_spec(compacted, excluded);
    let poster_w = chrome.width;
    let poster_h = chrome.height;
    let selected = chrome.selected;
    let busy = chrome.busy;
    let job = chrome.job;
    let progress_style = chrome.progress_style;
    let group = card_dom_id(&id);
    let job_active = job.is_some();
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
                    poster_shows_compacted_badge(compacted, excluded) && !job_active,
                    |el| el.child(render_compacted_badge(&id, &theme)),
                )
                .when_some(job, |el, job| {
                    el.child(render_poster_job_overlay(&id, job, &theme, progress_style))
                })
                .when(!job_active, |el| {
                    el.child(render_poster_veil(&id, group, hover_spec, busy, cx))
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

fn poster_hover_spec(compacted: bool, excluded: bool) -> PosterHoverSpec {
    if excluded {
        PosterHoverSpec {
            show_compress: false,
            show_decompress: false,
            show_change_method: false,
            show_play: true,
            show_excluded_hint: true,
        }
    } else if compacted {
        PosterHoverSpec {
            show_compress: false,
            show_decompress: true,
            show_change_method: true,
            show_play: true,
            show_excluded_hint: false,
        }
    } else {
        PosterHoverSpec {
            show_compress: true,
            show_decompress: false,
            show_change_method: false,
            show_play: true,
            show_excluded_hint: false,
        }
    }
}

fn poster_shows_compacted_badge(compacted: bool, excluded: bool) -> bool {
    compacted && !excluded
}

fn header_compact_label(selected_n: usize) -> Option<String> {
    (selected_n > 1).then(|| format!("Compress {selected_n}"))
}

fn poster_job_label(kind: PosterJobKind, waiting: bool) -> &'static str {
    if waiting {
        return "Waiting…";
    }
    match kind {
        PosterJobKind::Decompress => "Restoring",
        PosterJobKind::Change => "Changing",
        PosterJobKind::Compress => "Compressing",
        PosterJobKind::Walkback => "Walking back",
    }
}

fn poster_job_percent(job: &PosterJobView) -> f32 {
    if job.waiting || job.progress.total == 0 {
        return 0.0;
    }
    let pct = (job.progress.processed as f32 / job.progress.total as f32) * 100.0;
    if matches!(job.kind, PosterJobKind::Decompress) && job.progress.processed == 0 {
        return 28.0;
    }
    pct.clamp(0.0, 100.0)
}

fn render_poster_job_overlay(
    id: &str,
    job: PosterJobView,
    theme: &gpui_component::Theme,
    style: crate::settings::ProgressStyle,
) -> impl IntoElement {
    let label = poster_job_label(job.kind, job.waiting);
    let pct = poster_job_percent(&job);
    let message = if job.waiting {
        "Waiting…".to_string()
    } else {
        job.progress.message.clone()
    };
    v_flex()
        .id(SharedString::from(format!("poster-job-{id}")))
        .absolute()
        .inset_0()
        .justify_end()
        .p_2()
        .gap_1p5()
        .bg(hsla(0.54, 0.30, 0.04, 0.72))
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
        .child(
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
}

fn render_poster_veil(
    id: &str,
    group: SharedString,
    spec: PosterHoverSpec,
    busy: bool,
    cx: &mut Context<LibraryApp>,
) -> impl IntoElement {
    let theme = cx.theme().clone();
    let id = id.to_string();
    // Stay mounted and do not occlude: inserting an occluding veil on hover
    // steals the card hitbox, which drops hover and flickers the actions.
    v_flex()
        .id(SharedString::from(format!("poster-veil-{id}")))
        .absolute()
        .inset_0()
        .justify_end()
        .p_2()
        .gap_1p5()
        .bg(hsla(0.54, 0.30, 0.04, 0.52))
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
                    .icon(Icon::empty().path("icons/play.svg"))
                    .label("Play")
                    .tooltip("Play")
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
    use super::{
        header_compact_label, poster_hover_spec, poster_job_label, poster_job_percent,
        poster_job_view, poster_shows_compacted_badge, poster_size, PosterJob, PosterJobKind,
        PosterJobView, POSTER_GAP, POSTER_RADIUS, UNSELECTED_DIM,
    };
    use crate::compact::CompactProgress;
    use crate::library::LibraryStore;
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
    }

    #[test]
    fn uncompacted_hover_leads_with_compress() {
        let spec = poster_hover_spec(false, false);
        assert!(spec.show_compress);
        assert!(spec.show_play);
        assert!(!spec.show_decompress);
        assert!(!spec.show_change_method);
        assert!(!spec.show_excluded_hint);
        assert!(!poster_shows_compacted_badge(false, false));
    }

    #[test]
    fn compacted_hover_offers_decompress_and_change() {
        let spec = poster_hover_spec(true, false);
        assert!(!spec.show_compress);
        assert!(spec.show_decompress);
        assert!(spec.show_change_method);
        assert!(spec.show_play);
        assert!(poster_shows_compacted_badge(true, false));
        assert!(!poster_shows_compacted_badge(true, true));
    }

    #[test]
    fn excluded_hover_is_play_only() {
        let spec = poster_hover_spec(true, true);
        assert!(!spec.show_compress);
        assert!(!spec.show_decompress);
        assert!(!spec.show_change_method);
        assert!(spec.show_play);
        assert!(spec.show_excluded_hint);
    }

    #[test]
    fn header_compress_is_batch_only() {
        assert_eq!(header_compact_label(0), None);
        assert_eq!(header_compact_label(1), None);
        assert_eq!(header_compact_label(2).as_deref(), Some("Compress 2"));
        assert_eq!(header_compact_label(5).as_deref(), Some("Compress 5"));
    }

    #[test]
    fn poster_job_overlay_uses_file_progress_and_waiting() {
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
        let current = poster_job_view(Some(&job), "steam:1").unwrap();
        assert!(!current.waiting);
        assert!((poster_job_percent(&current) - 50.0).abs() < 0.01);
        assert_eq!(poster_job_label(current.kind, current.waiting), "Changing");
        let queued = poster_job_view(Some(&job), "steam:2").unwrap();
        assert!(queued.waiting);
        assert_eq!(poster_job_percent(&queued), 0.0);
        assert!(poster_job_view(Some(&job), "steam:3").is_none());

        let restoring = PosterJobView {
            kind: PosterJobKind::Decompress,
            waiting: false,
            progress: CompactProgress {
                processed: 0,
                total: 400,
                message: "Starting WOF uncompact…".into(),
            },
        };
        assert!(poster_job_percent(&restoring) > 0.0);
        assert_eq!(
            poster_job_label(PosterJobKind::Decompress, false),
            "Restoring"
        );
    }
}
