//! Change-method and Decompress dialogs. Compress uses the cinematic theater.
//!
//! One ~480px dialog. Radio-cards show Low / Medium / High / Maximum — no XPRESS/LZX.

use gpui::{
    div, prelude::FluentBuilder, px, AppContext, Context, InteractiveElement, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex, v_flex, ActiveTheme, Icon, Sizable, StyledExt, WindowExt,
};

use super::LibraryApp;
use crate::compact::{CompactLevel, CompactOp};
use crate::format::format_bytes;
use crate::library::LibraryTitle;

/// Why the Low / Medium / High picker is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompactPickerMode {
    Compress,
    ChangeMethod,
}

impl CompactPickerMode {
    pub(crate) fn dialog_title(self) -> &'static str {
        match self {
            Self::Compress => "Compress",
            Self::ChangeMethod => "Change method",
        }
    }

    pub(crate) fn confirm_label(self) -> &'static str {
        match self {
            Self::Compress => "Compress",
            Self::ChangeMethod => "Apply",
        }
    }

    pub(crate) fn heading(self, titles: &[LibraryTitle]) -> String {
        match (self, titles) {
            (Self::Compress, [title]) => format!("Compress {}.", title.name),
            (Self::Compress, _) => format!("Compress {} selected titles.", titles.len()),
            (Self::ChangeMethod, [title]) => format!("Change compression for {}.", title.name),
            (Self::ChangeMethod, _) => {
                format!("Change compression for {} selected titles.", titles.len())
            }
        }
    }
}

pub(crate) struct CompactLevelPicker {
    app: gpui::Entity<LibraryApp>,
    level: CompactLevel,
    heading: String,
    confirm_label: &'static str,
}

impl CompactLevelPicker {
    fn new(app: gpui::Entity<LibraryApp>, heading: String, confirm_label: &'static str) -> Self {
        Self {
            app,
            level: CompactLevel::Medium,
            heading,
            confirm_label,
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
                            .label(self.confirm_label)
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
    pub(crate) fn open_compact_picker(
        &mut self,
        mode: CompactPickerMode,
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
        let heading = mode.heading(&titles);
        let confirm_label = mode.confirm_label();
        let title = mode.dialog_title();
        let app = cx.entity();
        let picker = cx.new(|_cx| CompactLevelPicker::new(app, heading, confirm_label));
        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme().clone();
            dialog
                .title(title)
                .overlay_closable(true)
                .keyboard(true)
                .on_cancel(|_, _, _| true)
                .w(px(480.))
                .border_color(theme.border.opacity(0.32))
                .child(picker.clone())
        });
    }

    pub(crate) fn open_decompress_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let titles = self.selected_titles();
        if titles.is_empty() {
            self.show_toast("Select a game first.", cx);
            return;
        }
        if self.compact_busy {
            self.show_toast("A compact job is already running.", cx);
            return;
        }
        let heading = decompress_heading(&titles);
        let detail = decompress_detail(&titles);
        let app = cx.entity();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme().clone();
            dialog
                .title("Decompress")
                .overlay_closable(true)
                .keyboard(true)
                .on_cancel(|_, _, _| true)
                .w(px(420.))
                .border_color(theme.border.opacity(0.32))
                .child(
                    v_flex()
                        .id("decompress-confirm")
                        .gap_3()
                        .w_full()
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child(heading.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(detail.clone()),
                        )
                        .child(
                            h_flex()
                                .w_full()
                                .justify_end()
                                .gap_2()
                                .child(
                                    Button::new("decompress-confirm")
                                        .primary()
                                        .label("Decompress")
                                        .on_click({
                                            let app = app.clone();
                                            move |_, window, cx| {
                                                app.update(cx, |app, cx| {
                                                    app.start_compact(
                                                        CompactOp::Uncompress,
                                                        window,
                                                        cx,
                                                    );
                                                });
                                                window.close_dialog(cx);
                                            }
                                        }),
                                )
                                .child(
                                    Button::new("decompress-cancel")
                                        .outline()
                                        .label("Cancel")
                                        .on_click(|_, window, cx| {
                                            window.close_dialog(cx);
                                        }),
                                ),
                        ),
                )
        });
    }
}

fn decompress_heading(titles: &[LibraryTitle]) -> String {
    match titles {
        [title] => format!("Decompress {}?", title.name),
        _ => format!("Decompress {} selected titles?", titles.len()),
    }
}

fn decompress_detail(titles: &[LibraryTitle]) -> String {
    match titles {
        [title] => match title.logical_bytes {
            Some(logical) => format!(
                "Restores files toward {} on disk. The title stays playable.",
                format_bytes(logical)
            ),
            None => "Restores files to their uncompressed size. The title stays playable.".into(),
        },
        _ => "Restores files to their uncompressed size. Titles stay playable.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::library::LibraryStore;

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
    fn picker_copy_never_names_algorithms() {
        let one = [title(
            "Apex Legends",
            Some(75_000_000_000),
            Some(126_000_000),
        )];
        for mode in [CompactPickerMode::Compress, CompactPickerMode::ChangeMethod] {
            let blob = format!(
                "{} {} {} {}",
                mode.dialog_title(),
                mode.confirm_label(),
                mode.heading(&one),
                mode.heading(&one[..0])
            )
            .to_ascii_uppercase();
            assert!(!blob.contains("XPRESS"), "{blob}");
            assert!(!blob.contains("LZX"), "{blob}");
        }
        assert_eq!(CompactPickerMode::Compress.confirm_label(), "Compress");
        assert_eq!(CompactPickerMode::ChangeMethod.confirm_label(), "Apply");
        assert_eq!(
            CompactPickerMode::ChangeMethod.dialog_title(),
            "Change method"
        );
        assert_eq!(
            CompactPickerMode::ChangeMethod.heading(&one),
            "Change compression for Apex Legends."
        );
        assert_eq!(CompactLevel::ALL.len(), 4);
        assert_eq!(
            CompactLevel::High.algorithm(),
            crate::settings::CompactAlgorithm::Xpress16k
        );
        assert_eq!(
            CompactLevel::Maximum.algorithm(),
            crate::settings::CompactAlgorithm::Lzx
        );
    }

    #[test]
    fn decompress_copy_mentions_size_without_algorithms() {
        let one = [title(
            "Apex Legends",
            Some(75 * 1024 * 1024 * 1024),
            Some(126),
        )];
        let heading = decompress_heading(&one);
        let detail = decompress_detail(&one);
        assert!(heading.contains("Apex Legends"));
        assert!(detail.to_ascii_lowercase().contains("playable"));
        assert!(detail.contains("GB") || detail.contains("MB"));
        let blob = format!("{heading} {detail}").to_ascii_uppercase();
        assert!(!blob.contains("XPRESS"), "{blob}");
        assert!(!blob.contains("LZX"), "{blob}");
        assert_eq!(decompress_heading(&[]), "Decompress 0 selected titles?");
    }
}
