//! Confirm dialogs that must not use Switch (library Switch panics in dialogs).

use gpui::{div, Context, ParentElement, Styled, Window};
use gpui_component::{button::Button, h_flex, v_flex, ActiveTheme, Sizable, WindowExt};

use super::LibraryApp;
use crate::compact::CompactEstimate;
use crate::format::format_bytes;

impl LibraryApp {
    pub(crate) fn open_dstorage_warning(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        estimate: CompactEstimate,
    ) {
        let view = cx.entity();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme().clone();
            let view = view.clone();
            dialog
                .title("DirectStorage detected")
                .confirm()
                .child(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().child(
                            "This install contains dstorage.dll or dstoragecore.dll. Compacting it can break DirectStorage I/O.",
                        ))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(format!(
                                    "Estimate: {} → ~{} ({} files, {} skipped).",
                                    format_bytes(estimate.logical_bytes),
                                    format_bytes(estimate.estimated_on_disk_bytes),
                                    estimate.file_count,
                                    estimate.skipped_count
                                )),
                        )
                        .child(
                            h_flex().child(
                                Button::new("dstorage-enable-override")
                                    .outline()
                                    .small()
                                    .label("Enable override in Settings")
                                    .on_click({
                                        let view = view.clone();
                                        move |_, _window, cx| {
                                            view.update(cx, |app, cx| {
                                                app.settings.allow_dstorage_override = true;
                                                app.show_toast(
                                                    "Override enabled in the live draft. Save settings to persist.",
                                                    cx,
                                                );
                                            });
                                        }
                                    }),
                            ),
                        ),
                )
        });
    }
}
