//! Selected-title inspector: sizes and compact actions stay on screen.

use gpui::{
    div, img, prelude::FluentBuilder, px, Context, InteractiveElement, IntoElement, ObjectFit,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, StyledImage,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex, v_flex, ActiveTheme, Disableable, Icon, Sizable, StyledExt,
};

use super::compact_apply::TitleActivity;
use super::widgets::{shorten_path_display, styled_progress};
use super::LibraryApp;
use crate::appearance::title_tint;
use crate::covers::Monogram;
use crate::format::{format_bytes, format_date};
use crate::library::{LibraryStore, LibraryTitle};
use crate::settings::UiDensity;

const INSPECTOR_W_COMFORTABLE: f32 = 312.0;
const INSPECTOR_W_COMPACT: f32 = 280.0;
const COVER_W: f32 = 168.0;
const COVER_H: f32 = 252.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InspectorView {
    Empty,
    Single(LibraryTitle),
    Batch(Vec<LibraryTitle>),
}

impl InspectorView {
    pub(crate) fn from_titles(titles: Vec<LibraryTitle>) -> Self {
        let mut iter = titles.into_iter();
        match (iter.next(), iter.next()) {
            (None, _) => Self::Empty,
            (Some(one), None) => Self::Single(one),
            (Some(one), Some(two)) => {
                let mut rest = vec![one, two];
                rest.extend(iter);
                Self::Batch(rest)
            }
        }
    }

    pub(crate) fn batch_compress_label(&self) -> Option<String> {
        match self {
            Self::Batch(titles) if titles.len() > 1 => Some(format!("Compress {}", titles.len())),
            _ => None,
        }
    }

    pub(crate) fn batch_logical_bytes(&self) -> Option<u64> {
        match self {
            Self::Batch(titles) => {
                let sum: u64 = titles.iter().filter_map(|g| g.logical_bytes).sum();
                (sum > 0).then_some(sum)
            }
            _ => None,
        }
    }
}

pub(crate) fn inspector_width(density: UiDensity) -> f32 {
    match density {
        UiDensity::Comfortable => INSPECTOR_W_COMFORTABLE,
        UiDensity::Compact => INSPECTOR_W_COMPACT,
    }
}

impl LibraryApp {
    pub(crate) fn render_inspector(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let view = InspectorView::from_titles(self.selected_titles());
        let busy = self.compact_busy || self.compact_flow.is_some();
        let poster_job = self.poster_job.clone();
        let live = self.live.clone();

        div()
            .id("library-inspector")
            .size_full()
            .min_h_0()
            .overflow_hidden()
            .bg(theme.sidebar)
            .border_l_1()
            .border_color(theme.sidebar_border)
            .child(
                div()
                    .id("library-inspector-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .p_4()
                    .child(match view {
                        InspectorView::Empty => self.render_inspector_empty(cx).into_any_element(),
                        InspectorView::Single(game) => {
                            let activity =
                                TitleActivity::resolve(&game, poster_job.as_ref(), &live);
                            self.render_inspector_single(game, activity, busy, cx)
                                .into_any_element()
                        }
                        InspectorView::Batch(titles) => self
                            .render_inspector_batch(titles, busy, cx)
                            .into_any_element(),
                    }),
            )
    }

    fn render_inspector_empty(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        v_flex()
            .id("inspector-empty")
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .px_2()
            .child(
                div()
                    .text_sm()
                    .font_semibold()
                    .text_color(theme.foreground)
                    .child("Select a game"),
            )
            .child(
                div()
                    .text_xs()
                    .text_center()
                    .text_color(theme.muted_foreground)
                    .child("Compress, restore, and play stay here. Ctrl-click adds to the set."),
            )
    }

    fn render_inspector_single(
        &mut self,
        game: LibraryTitle,
        activity: TitleActivity,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme().clone();
        let id = game.id.clone();
        let cover = self.cover_image(&id);
        let excluded = matches!(activity, TitleActivity::Excluded);
        let compacted = game.is_compacted();
        let launch = game.store.launch_label();
        let allows_compact = activity.allows_compact();
        let progress_style = self.settings.progress_style;
        let path = game.install_path.display().to_string();
        let short_path = shorten_path_display(&path);
        let last_played = game.last_played_unix.map(format_date);
        let has_art = cover.is_some();
        let tint = title_tint(&game.name, theme.is_dark());
        let monogram = Monogram::from_title(&game);
        let radius = theme.radius_lg;

        v_flex()
            .id(SharedString::from(format!("inspector-single-{id}")))
            .w_full()
            .gap_3()
            .child(
                div()
                    .id(SharedString::from(format!("inspector-cover-{id}")))
                    .relative()
                    .mx_auto()
                    .w(px(COVER_W))
                    .h(px(COVER_H))
                    .rounded(radius)
                    .overflow_hidden()
                    .bg(if has_art { theme.secondary } else { tint })
                    .child(if let Some(image) = cover {
                        img(image)
                            .absolute()
                            .inset_0()
                            .size_full()
                            .object_fit(ObjectFit::Cover)
                            .into_any_element()
                    } else {
                        v_flex()
                            .size_full()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .text_3xl()
                                    .font_bold()
                                    .text_color(theme.foreground)
                                    .child(monogram.initials.clone()),
                            )
                            .into_any_element()
                    }),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_lg()
                            .font_bold()
                            .text_color(theme.foreground)
                            .child(game.name.clone()),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(game.store.badge()),
                            )
                            .when_some(activity.heading(), |el, heading| {
                                el.child(
                                    div()
                                        .text_xs()
                                        .font_semibold()
                                        .text_color(match activity {
                                            TitleActivity::Patching => theme.warning,
                                            TitleActivity::Job { .. } => theme.primary,
                                            TitleActivity::Excluded => theme.muted_foreground,
                                            TitleActivity::Idle => theme.primary,
                                        })
                                        .child(heading),
                                )
                            })
                            .when(compacted && matches!(activity, TitleActivity::Idle), |el| {
                                el.child(
                                    div()
                                        .text_xs()
                                        .font_semibold()
                                        .text_color(theme.primary)
                                        .child("Compacted"),
                                )
                            }),
                    )
                    .when_some(last_played, |el, when| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(format!("Played {when}")),
                        )
                    }),
            )
            .child(render_size_block(&game, &theme))
            .child(
                h_flex()
                    .id(SharedString::from(format!("inspector-path-{id}")))
                    .gap_2()
                    .items_center()
                    .cursor_pointer()
                    .on_click({
                        let id = id.clone();
                        cx.listener(move |this, _, _, cx| {
                            this.open_install_folder(&id, cx);
                        })
                    })
                    .child(
                        Icon::empty()
                            .path("icons/folder.svg")
                            .with_size(px(14.))
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .truncate()
                            .child(short_path),
                    ),
            )
            .when_some(activity.detail(), |el, detail| {
                el.child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(detail),
                )
            })
            .when_some(activity.percent(), |el, pct| {
                el.child(div().w_full().child(styled_progress(
                    pct,
                    theme.progress_bar,
                    progress_style,
                )))
            })
            .when(allows_compact && !compacted, |el| {
                el.child(
                    Button::new(SharedString::from(format!("inspector-compress-{id}")))
                        .primary()
                        .w_full()
                        .icon(Icon::empty().path("icons/file-archive.svg"))
                        .label("Compress")
                        .disabled(busy)
                        .on_click({
                            let id = id.clone();
                            cx.listener(move |this, _, window, cx| {
                                this.begin_title_compress(id.clone(), window, cx);
                            })
                        }),
                )
            })
            .when(allows_compact && compacted, |el| {
                el.child(
                    Button::new(SharedString::from(format!("inspector-decompress-{id}")))
                        .outline()
                        .w_full()
                        .icon(Icon::empty().path("icons/undo-2.svg"))
                        .label("Decompress")
                        .disabled(busy)
                        .on_click({
                            let id = id.clone();
                            cx.listener(move |this, _, window, cx| {
                                this.begin_title_decompress(id.clone(), window, cx);
                            })
                        }),
                )
                .child(
                    Button::new(SharedString::from(format!("inspector-change-{id}")))
                        .outline()
                        .w_full()
                        .icon(Icon::empty().path("icons/redo-2.svg"))
                        .label("Change method")
                        .disabled(busy)
                        .on_click({
                            let id = id.clone();
                            cx.listener(move |this, _, window, cx| {
                                this.begin_title_change_method(id.clone(), window, cx);
                            })
                        }),
                )
            })
            .child(
                Button::new(SharedString::from(format!("inspector-launch-{id}")))
                    .when(excluded || compacted, |b| b.primary())
                    .when(!excluded && !compacted, |b| b.outline())
                    .w_full()
                    .icon(Icon::empty().path(game.store.launch_icon_path()))
                    .label(launch)
                    .disabled(busy)
                    .on_click({
                        let id = id.clone();
                        cx.listener(move |this, _, window, cx| {
                            this.select_game(id.clone(), cx);
                            this.launch_selected(window, cx);
                        })
                    }),
            )
            .when(game.store == LibraryStore::Steam, |el| {
                el.child(
                    Button::new(SharedString::from(format!("inspector-folder-{id}")))
                        .ghost()
                        .w_full()
                        .icon(Icon::empty().path("icons/folder-open.svg"))
                        .label("Open folder")
                        .on_click({
                            let id = id.clone();
                            cx.listener(move |this, _, _, cx| {
                                this.open_install_folder(&id, cx);
                            })
                        }),
                )
            })
    }

    fn render_inspector_batch(
        &mut self,
        titles: Vec<LibraryTitle>,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme().clone();
        let view = InspectorView::Batch(titles.clone());
        let n = titles.len();
        let label = view
            .batch_compress_label()
            .unwrap_or_else(|| format!("Compress {n}"));
        let logical = view.batch_logical_bytes();

        v_flex()
            .id("inspector-batch")
            .w_full()
            .gap_3()
            .child(
                div()
                    .text_lg()
                    .font_bold()
                    .text_color(theme.foreground)
                    .child(format!("{n} selected")),
            )
            .when_some(logical, |el, bytes| {
                el.child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(format!("{} logical", format_bytes(bytes))),
                )
            })
            .children(titles.into_iter().take(8).map(|game| {
                div()
                    .text_sm()
                    .text_color(theme.foreground)
                    .truncate()
                    .child(game.name)
            }))
            .child(
                Button::new("inspector-batch-compress")
                    .primary()
                    .w_full()
                    .icon(Icon::empty().path("icons/file-archive.svg"))
                    .label(label)
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_compact_flow(window, cx);
                    })),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("Ctrl-click a poster to add or remove it."),
            )
    }
}

fn render_size_block(game: &LibraryTitle, theme: &gpui_component::Theme) -> impl IntoElement {
    let saved = game.saved_bytes().map(format_bytes);
    let block = v_flex().id("inspector-sizes").w_full().gap_1();
    match (game.on_disk_bytes, game.logical_bytes) {
        (Some(disk), Some(logical)) if disk < logical => block
            .child(stat_row(
                "On disk",
                format_bytes(disk),
                theme.foreground,
                theme,
            ))
            .child(stat_row(
                "Logical",
                format_bytes(logical),
                theme.foreground,
                theme,
            ))
            .when_some(saved, |el, saved| {
                el.child(stat_row("Saved", saved, theme.primary, theme))
            }),
        (Some(disk), _) => block.child(stat_row(
            "On disk",
            format_bytes(disk),
            theme.foreground,
            theme,
        )),
        (None, Some(logical)) => block.child(stat_row(
            "Size",
            format_bytes(logical),
            theme.foreground,
            theme,
        )),
        (None, None) => block.child(stat_row(
            "Size",
            "Unknown".into(),
            theme.muted_foreground,
            theme,
        )),
    }
}

fn stat_row(
    label: &'static str,
    value: String,
    value_color: gpui::Hsla,
    theme: &gpui_component::Theme,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(label),
        )
        .child(
            div()
                .text_sm()
                .font_semibold()
                .text_color(value_color)
                .child(value),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::LibraryStore;
    use std::path::PathBuf;
    use stores::StoreId;

    fn title(
        store: LibraryStore,
        name: &str,
        logical: Option<u64>,
        disk: Option<u64>,
    ) -> LibraryTitle {
        LibraryTitle {
            id: format!("{}:{name}", store.as_str()),
            name: name.into(),
            install_path: PathBuf::from(r"D:\Games").join(name),
            store,
            launcher_id: None,
            last_played_unix: None,
            logical_bytes: logical,
            on_disk_bytes: disk,
            compacted: disk.is_some_and(|d| logical.is_some_and(|l| d < l)),
            steam_app_id: None,
            steam_library_path: None,
            steam_install_dir_name: None,
            cover_url: None,
        }
    }

    #[test]
    fn empty_selection_is_empty_inspector() {
        assert_eq!(InspectorView::from_titles(vec![]), InspectorView::Empty);
    }

    #[test]
    fn one_title_is_single() {
        let game = title(LibraryStore::Steam, "Hades", Some(40), Some(18));
        match InspectorView::from_titles(vec![game.clone()]) {
            InspectorView::Single(got) => assert_eq!(got.id, game.id),
            other => panic!("expected single, got {other:?}"),
        }
    }

    #[test]
    fn many_titles_are_batch_with_compress_label() {
        let titles = vec![
            title(LibraryStore::Steam, "Hades", Some(10), Some(10)),
            title(
                LibraryStore::Extra(StoreId::Gog),
                "Celeste",
                Some(5),
                Some(5),
            ),
            title(LibraryStore::Custom, "Portable", None, None),
        ];
        let view = InspectorView::from_titles(titles);
        assert_eq!(view.batch_compress_label().as_deref(), Some("Compress 3"));
        assert_eq!(view.batch_logical_bytes(), Some(15));
    }

    #[test]
    fn inspector_width_tracks_density() {
        assert!(inspector_width(UiDensity::Comfortable) > inspector_width(UiDensity::Compact));
        assert!((300.0..320.0).contains(&inspector_width(UiDensity::Comfortable)));
    }

    #[test]
    fn cover_is_portrait_2_by_3() {
        assert!((COVER_W * 3.0 - COVER_H * 2.0).abs() < 0.01);
    }
}
