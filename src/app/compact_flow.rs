//! Full-viewport compress theater: choose strength, watch progress, reveal stats.
//!
//! Clicking a level card starts immediately. After the job finishes, any click
//! dismisses the overlay and fades back to the library.

use std::time::Duration;

use gpui::{
    div, ease_out_quint, img, prelude::FluentBuilder, pulsating_between, px, relative, Animation,
    AnimationExt, ClickEvent, Context, InteractiveElement, IntoElement, ObjectFit, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, StyledImage, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex, v_flex, ActiveTheme, Icon, Sizable, StyledExt,
};

use super::widgets::styled_progress;
use super::LibraryApp;
use crate::compact::{CompactLevel, CompactProgress, CompactSizeSnapshot};
use crate::format::format_bytes;
use crate::library::LibraryTitle;

pub(crate) const FLOW_FADE: Duration = Duration::from_millis(320);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompactFlowPhase {
    Choose,
    Working,
    Done,
    Leaving,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TitleCompactStats {
    pub id: String,
    pub name: String,
    pub before: CompactSizeSnapshot,
    pub after: CompactSizeSnapshot,
}

impl TitleCompactStats {
    pub fn reclaimed_bytes(&self) -> u64 {
        self.before
            .on_disk_bytes
            .saturating_sub(self.after.on_disk_bytes)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CompactFlow {
    pub titles: Vec<LibraryTitle>,
    pub phase: CompactFlowPhase,
    pub selected_level: CompactLevel,
    pub progress: Option<CompactProgress>,
    pub stats: Vec<TitleCompactStats>,
    pub failed: bool,
    pub finish_message: String,
    pub anim_gen: u64,
}

impl CompactFlow {
    fn new(titles: Vec<LibraryTitle>) -> Self {
        Self {
            titles,
            phase: CompactFlowPhase::Choose,
            selected_level: CompactLevel::Medium,
            progress: None,
            stats: Vec::new(),
            failed: false,
            finish_message: String::new(),
            anim_gen: 0,
        }
    }

    fn cover_id(&self) -> Option<&str> {
        self.titles.first().map(|t| t.id.as_str())
    }

    fn can_cancel(&self) -> bool {
        matches!(self.phase, CompactFlowPhase::Choose)
    }

    fn can_dismiss(&self) -> bool {
        matches!(
            self.phase,
            CompactFlowPhase::Done | CompactFlowPhase::Leaving
        )
    }

    fn visual_phase(&self) -> CompactFlowPhase {
        match self.phase {
            CompactFlowPhase::Leaving => CompactFlowPhase::Done,
            other => other,
        }
    }
}

impl LibraryApp {
    pub(crate) fn begin_title_compress(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_game(id, cx);
        self.open_compact_flow(window, cx);
    }

    pub(crate) fn open_compact_flow(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.compact_flow.is_some() || self.compact_busy {
            self.show_toast("A compact job is already running.", cx);
            return;
        }
        let titles = self.selected_titles();
        if titles.is_empty() {
            self.show_toast("Select a game first.", cx);
            return;
        }
        self.compact_flow = Some(CompactFlow::new(titles));
        cx.notify();
    }

    pub(crate) fn dismiss_compact_flow(&mut self, cx: &mut Context<Self>) {
        let Some(flow) = self.compact_flow.as_mut() else {
            return;
        };
        if matches!(
            flow.phase,
            CompactFlowPhase::Working | CompactFlowPhase::Leaving
        ) {
            return;
        }
        flow.phase = CompactFlowPhase::Leaving;
        flow.anim_gen = flow.anim_gen.saturating_add(1);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FLOW_FADE).await;
            let _ = this.update(cx, |app, cx| {
                if app
                    .compact_flow
                    .as_ref()
                    .is_some_and(|f| f.phase == CompactFlowPhase::Leaving)
                {
                    app.compact_flow = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(crate) fn dismiss_compact_flow_now(&mut self, cx: &mut Context<Self>) {
        if self.compact_flow.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn choose_compact_level(
        &mut self,
        level: CompactLevel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self
            .compact_flow
            .as_ref()
            .is_some_and(|f| f.phase == CompactFlowPhase::Choose)
        {
            return;
        }
        if let Some(flow) = self.compact_flow.as_mut() {
            flow.selected_level = level;
        }
        self.apply_compact_level(level, window, cx);
    }

    pub(crate) fn render_compact_flow(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(flow) = self.compact_flow.clone() else {
            return div().id("compact-flow-idle").into_any_element();
        };
        let theme = cx.theme().clone();
        let leaving = flow.phase == CompactFlowPhase::Leaving;
        let dismissable = flow.can_dismiss();
        let cover = flow.cover_id().and_then(|id| self.cover_image(id));
        let visual = flow.visual_phase();
        let overlay_id = if leaving {
            SharedString::from("compact-flow-leave")
        } else {
            SharedString::from(format!("compact-flow-enter-{}", flow.anim_gen))
        };
        let stage_id = SharedString::from(format!(
            "compact-flow-stage-{}-{}",
            phase_key(visual),
            flow.anim_gen
        ));
        let progress_style = self.settings.progress_style;
        let hint = flow_hint(visual, flow.failed);
        let heading = flow_heading(&flow.titles, visual);

        div()
            .id("compact-flow-root")
            .absolute()
            .inset_0()
            .size_full()
            .occlude()
            .when(dismissable, |el| {
                el.cursor_pointer()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.dismiss_compact_flow(cx);
                    }))
            })
            .child(
                div()
                    .id(overlay_id)
                    .size_full()
                    .relative()
                    .overflow_hidden()
                    .bg(theme.background.opacity(0.88))
                    .when_some(cover, |el, image| {
                        el.child(
                            img(image)
                                .absolute()
                                .inset_0()
                                .size_full()
                                .object_fit(ObjectFit::Cover)
                                .opacity(0.16),
                        )
                    })
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .bg(theme.background.opacity(0.55)),
                    )
                    .child(
                        v_flex()
                            .id("compact-flow-stage-wrap")
                            .size_full()
                            .items_center()
                            .justify_center()
                            .p_8()
                            .child(
                                v_flex()
                                    .id(stage_id)
                                    .w_full()
                                    .max_w(px(680.))
                                    .items_center()
                                    .gap_5()
                                    .child(
                                        div()
                                            .text_xl()
                                            .font_bold()
                                            .text_color(theme.foreground)
                                            .child(heading),
                                    )
                                    .child(match visual {
                                        CompactFlowPhase::Choose => {
                                            self.render_flow_choose(&flow, cx).into_any_element()
                                        }
                                        CompactFlowPhase::Working => render_flow_working(
                                            &flow,
                                            theme.muted_foreground,
                                            theme.progress_bar,
                                            progress_style,
                                        )
                                        .into_any_element(),
                                        CompactFlowPhase::Done | CompactFlowPhase::Leaving => {
                                            render_flow_done(&flow, cx).into_any_element()
                                        }
                                    })
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.muted_foreground)
                                            .child(hint),
                                    )
                                    .when(flow.can_cancel(), |el| {
                                        el.child(
                                            Button::new("compact-flow-cancel")
                                                .outline()
                                                .label("Cancel")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.dismiss_compact_flow(cx);
                                                })),
                                        )
                                    })
                                    .with_animation(
                                        SharedString::from(format!(
                                            "compact-stage-anim-{}-{}",
                                            phase_key(visual),
                                            flow.anim_gen
                                        )),
                                        Animation::new(FLOW_FADE).with_easing(ease_out_quint()),
                                        |this, delta| {
                                            this.opacity(delta).mt(px(18.0 * (1.0 - delta)))
                                        },
                                    ),
                            ),
                    )
                    .with_animation(
                        SharedString::from(if leaving {
                            "compact-overlay-leave"
                        } else {
                            "compact-overlay-enter"
                        }),
                        Animation::new(FLOW_FADE).with_easing(ease_out_quint()),
                        move |this, delta| this.opacity(if leaving { 1.0 - delta } else { delta }),
                    ),
            )
            .into_any_element()
    }

    fn render_flow_choose(&self, flow: &CompactFlow, cx: &mut Context<Self>) -> impl IntoElement {
        let logical = titles_logical(&flow.titles);
        v_flex()
            .id("compact-flow-choose")
            .w_full()
            .gap_2()
            .child(self.render_flow_choose_row(
                0,
                [CompactLevel::Low, CompactLevel::Medium],
                flow.selected_level,
                logical,
                cx,
            ))
            .child(self.render_flow_choose_row(
                1,
                [CompactLevel::High, CompactLevel::Maximum],
                flow.selected_level,
                logical,
                cx,
            ))
    }

    fn render_flow_choose_row(
        &self,
        row: usize,
        levels: [CompactLevel; 2],
        selected: CompactLevel,
        logical: Option<u64>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme().clone();
        h_flex()
            .id(SharedString::from(format!("compact-flow-row-{row}")))
            .w_full()
            .gap_2()
            .children(levels.into_iter().map(|level| {
                let on = selected == level;
                let saved = estimated_saved(logical, level);
                h_flex()
                    .id(SharedString::from(format!(
                        "compact-flow-level-{}",
                        level.label()
                    )))
                    .flex_1()
                    .h(px(108.))
                    .px_4()
                    .py_3()
                    .gap_3()
                    .items_center()
                    .rounded(px(12.))
                    .border_1()
                    .border_color(if on {
                        theme.primary
                    } else {
                        theme.border.opacity(0.45)
                    })
                    .bg(if on {
                        theme
                            .primary
                            .opacity(if theme.is_dark() { 0.16 } else { 0.10 })
                    } else {
                        theme.secondary.opacity(0.28)
                    })
                    .hover(|s| s.bg(theme.secondary.opacity(0.5)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        cx.stop_propagation();
                        this.choose_compact_level(level, window, cx);
                    }))
                    .child(
                        Icon::empty()
                            .path(level.icon_path())
                            .with_size(px(20.))
                            .text_color(if on {
                                theme.foreground
                            } else {
                                theme.muted_foreground
                            }),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .flex_1()
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_2()
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
                                                .bg(theme.primary.opacity(0.22))
                                                .text_xs()
                                                .text_color(theme.primary)
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
                            .when_some(saved, |el, bytes| {
                                el.child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.foreground.opacity(0.86))
                                        .child(format!("About {} back", format_bytes(bytes))),
                                )
                            }),
                    )
            }))
    }
}

fn render_flow_working(
    flow: &CompactFlow,
    muted: gpui::Hsla,
    bar: gpui::Hsla,
    style: crate::settings::ProgressStyle,
) -> impl IntoElement {
    let progress = flow.progress.clone();
    let pct = progress
        .as_ref()
        .map(|p| {
            if p.total == 0 {
                0.0
            } else {
                (p.processed as f32 / p.total as f32) * 100.0
            }
        })
        .unwrap_or(0.0);
    let message = progress
        .as_ref()
        .map(|p| p.message.clone())
        .unwrap_or_else(|| "Starting…".into());
    v_flex()
        .id("compact-flow-working")
        .w_full()
        .max_w(px(520.))
        .gap_3()
        .items_center()
        .child(div().text_sm().text_color(muted).child(format!(
            "{} · {}",
            flow.selected_level.label(),
            message
        )))
        .child(
            div()
                .w_full()
                .child(styled_progress(pct, bar, style))
                .with_animation(
                    "compact-flow-bar-pulse",
                    Animation::new(Duration::from_secs(2))
                        .repeat()
                        .with_easing(pulsating_between(0.72, 1.0)),
                    |this, delta| this.opacity(delta),
                ),
        )
}

fn render_flow_done(flow: &CompactFlow, cx: &mut Context<LibraryApp>) -> impl IntoElement {
    let theme = cx.theme().clone();
    let (before_disk, after_disk, reclaimed) = aggregate_stats(&flow.stats);
    let hero = done_hero(reclaimed, flow.failed, !flow.stats.is_empty());
    let fill = if before_disk == 0 {
        1.0
    } else {
        after_disk as f32 / before_disk as f32
    };
    v_flex()
        .id("compact-flow-done")
        .w_full()
        .max_w(px(560.))
        .items_center()
        .gap_4()
        .child(
            v_flex()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(if flow.failed {
                            "Finished with errors"
                        } else {
                            "Saved"
                        }),
                )
                .child(
                    div()
                        .text_3xl()
                        .font_bold()
                        .text_color(if flow.failed {
                            theme.danger
                        } else {
                            theme.foreground
                        })
                        .child(hero),
                )
                .when(!flow.finish_message.is_empty(), |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(flow.finish_message.clone()),
                    )
                }),
        )
        .when(before_disk > 0, |el| {
            el.child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        div()
                            .w_full()
                            .h(px(10.))
                            .rounded_full()
                            .bg(theme.secondary.opacity(0.55))
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(fill.clamp(0.06, 1.0)))
                                    .rounded_full()
                                    .bg(theme.primary),
                            ),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .child(stat_col(
                                "Before",
                                before_disk,
                                theme.muted_foreground,
                                theme.foreground,
                            ))
                            .child(stat_col(
                                "After",
                                after_disk,
                                theme.muted_foreground,
                                theme.foreground,
                            )),
                    ),
            )
        })
        .when(flow.stats.len() > 1, |el| {
            el.child(
                v_flex()
                    .w_full()
                    .gap_1()
                    .children(flow.stats.iter().map(|stat| {
                        h_flex()
                            .id(SharedString::from(format!("compact-stat-{}", stat.id)))
                            .w_full()
                            .justify_between()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(stat.name.clone())
                            .child(format!(
                                "{} → {}",
                                format_bytes(stat.before.on_disk_bytes),
                                format_bytes(stat.after.on_disk_bytes)
                            ))
                    })),
            )
        })
        .child(
            Button::new("compact-flow-done")
                .primary()
                .label("Done")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.dismiss_compact_flow(cx);
                })),
        )
}

fn stat_col(
    label: &'static str,
    bytes: u64,
    muted: gpui::Hsla,
    fg: gpui::Hsla,
) -> impl IntoElement {
    v_flex()
        .gap_0p5()
        .child(div().text_xs().text_color(muted).child(label))
        .child(
            div()
                .text_sm()
                .font_semibold()
                .text_color(fg)
                .child(format!("{} on disk", format_bytes(bytes))),
        )
}

fn phase_key(phase: CompactFlowPhase) -> &'static str {
    match phase {
        CompactFlowPhase::Choose => "choose",
        CompactFlowPhase::Working => "working",
        CompactFlowPhase::Done => "done",
        CompactFlowPhase::Leaving => "leave",
    }
}

fn flow_heading(titles: &[LibraryTitle], phase: CompactFlowPhase) -> String {
    match (phase, titles) {
        (CompactFlowPhase::Choose, [title]) => format!("Compress {}", title.name),
        (CompactFlowPhase::Choose, _) => format!("Compress {} titles", titles.len()),
        (CompactFlowPhase::Working, [title]) => title.name.clone(),
        (CompactFlowPhase::Working, _) => format!("Compressing {} titles", titles.len()),
        (CompactFlowPhase::Done | CompactFlowPhase::Leaving, [title]) => title.name.clone(),
        (CompactFlowPhase::Done | CompactFlowPhase::Leaving, _) => {
            format!("{} titles", titles.len())
        }
    }
}

fn flow_hint(phase: CompactFlowPhase, failed: bool) -> &'static str {
    match phase {
        CompactFlowPhase::Choose => "Pick a strength to start. You can cancel until then.",
        CompactFlowPhase::Working => "Keep this window open until compression finishes.",
        CompactFlowPhase::Done | CompactFlowPhase::Leaving if failed => {
            "Click anywhere to return to the library."
        }
        CompactFlowPhase::Done | CompactFlowPhase::Leaving => {
            "Click anywhere or any button to return."
        }
    }
}

fn titles_logical(titles: &[LibraryTitle]) -> Option<u64> {
    let mut sum = 0u64;
    let mut any = false;
    for title in titles {
        if let Some(n) = title.logical_bytes {
            sum = sum.saturating_add(n);
            any = true;
        }
    }
    any.then_some(sum)
}

fn estimated_saved(logical: Option<u64>, level: CompactLevel) -> Option<u64> {
    let logical = logical.filter(|n| *n > 0)?;
    let after = (logical as f64 * level.estimate_ratio()).round() as u64;
    Some(logical.saturating_sub(after))
}

fn aggregate_stats(stats: &[TitleCompactStats]) -> (u64, u64, u64) {
    let before = stats.iter().map(|s| s.before.on_disk_bytes).sum();
    let after = stats.iter().map(|s| s.after.on_disk_bytes).sum();
    let reclaimed = stats.iter().map(|s| s.reclaimed_bytes()).sum();
    (before, after, reclaimed)
}

fn done_hero(reclaimed: u64, failed: bool, has_stats: bool) -> String {
    if failed && !has_stats {
        "Couldn't finish".into()
    } else if reclaimed == 0 {
        "On disk unchanged".into()
    } else {
        format_bytes(reclaimed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::LibraryStore;
    use std::path::PathBuf;

    fn title(name: &str, logical: Option<u64>, on_disk: Option<u64>) -> LibraryTitle {
        LibraryTitle {
            id: format!("steam:{name}"),
            name: name.into(),
            install_path: PathBuf::from(r"C:\games\Title"),
            store: LibraryStore::Steam,
            launcher_id: Some("1".into()),
            last_played_unix: None,
            logical_bytes: logical,
            on_disk_bytes: on_disk,
            compacted: crate::library::sizes_indicate_compacted(on_disk, logical),
            steam_app_id: Some(1),
            steam_library_path: None,
            steam_install_dir_name: None,
            cover_url: None,
        }
    }

    #[test]
    fn copy_never_names_algorithms() {
        let one = [title(
            "Apex Legends",
            Some(75_000_000_000),
            Some(40_000_000_000),
        )];
        let mut blob = String::new();
        for phase in [
            CompactFlowPhase::Choose,
            CompactFlowPhase::Working,
            CompactFlowPhase::Done,
        ] {
            blob.push_str(&flow_heading(&one, phase));
            blob.push(' ');
            blob.push_str(flow_hint(phase, false));
        }
        for level in CompactLevel::ALL {
            blob.push_str(level.label());
            blob.push_str(level.tradeoff());
            if let Some(saved) = estimated_saved(Some(75_000_000_000), level) {
                blob.push_str(&format!("About {} back", format_bytes(saved)));
            }
        }
        blob.push_str(&done_hero(18_000_000_000, false, true));
        let upper = blob.to_ascii_uppercase();
        assert!(!upper.contains("XPRESS"), "{blob}");
        assert!(!upper.contains("LZX"), "{blob}");
        assert!(flow_heading(&one, CompactFlowPhase::Choose).contains("Apex Legends"));
        assert_eq!(
            flow_heading(&[], CompactFlowPhase::Choose),
            "Compress 0 titles"
        );
    }

    #[test]
    fn choose_is_cancellable_done_is_dismissable() {
        let flow = CompactFlow::new(vec![title("Apex Legends", Some(10), Some(10))]);
        assert!(flow.can_cancel());
        assert!(!flow.can_dismiss());
        let mut working = flow.clone();
        working.phase = CompactFlowPhase::Working;
        assert!(!working.can_cancel());
        assert!(!working.can_dismiss());
        let mut done = flow;
        done.phase = CompactFlowPhase::Done;
        assert!(!done.can_cancel());
        assert!(done.can_dismiss());
    }

    #[test]
    fn estimated_saved_shrinks_with_strength() {
        let logical = Some(100_000u64);
        let low = estimated_saved(logical, CompactLevel::Low).unwrap();
        let mid = estimated_saved(logical, CompactLevel::Medium).unwrap();
        let high = estimated_saved(logical, CompactLevel::High).unwrap();
        let max = estimated_saved(logical, CompactLevel::Maximum).unwrap();
        assert!(low < mid && mid < high && high < max);
        assert!(estimated_saved(None, CompactLevel::Medium).is_none());
        assert!(estimated_saved(Some(0), CompactLevel::Medium).is_none());
    }

    #[test]
    fn aggregate_and_hero_cover_empty_and_zero() {
        assert_eq!(aggregate_stats(&[]), (0, 0, 0));
        assert_eq!(done_hero(0, true, false), "Couldn't finish");
        assert_eq!(done_hero(0, false, true), "On disk unchanged");
        assert!(done_hero(2 * 1024 * 1024, false, true).contains("MB"));
        let stat = TitleCompactStats {
            id: "steam:1".into(),
            name: "Apex".into(),
            before: CompactSizeSnapshot {
                logical_bytes: 20,
                on_disk_bytes: 20,
                file_count: 2,
            },
            after: CompactSizeSnapshot {
                logical_bytes: 20,
                on_disk_bytes: 8,
                file_count: 2,
            },
        };
        assert_eq!(stat.reclaimed_bytes(), 12);
        assert_eq!(aggregate_stats(&[stat]), (20, 8, 12));
    }
}
