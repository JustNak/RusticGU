//! General settings category panel.

use gpui::{
    div, prelude::FluentBuilder, Context, IntoElement, ParentElement, SharedString, Styled,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    clipboard::Clipboard,
    group_box::{GroupBox, GroupBoxVariants},
    h_flex, v_flex, ActiveTheme, Disableable, Icon, IconName, Sizable,
};

use super::super::widgets::{
    field_hint, settings_choice_row, settings_field_label, settings_subgroup, shorten_path_display,
};
use super::super::LibraryApp;
use crate::settings::{CompactAlgorithm, UpdateChannel};

impl LibraryApp {
    pub(super) fn render_settings_general(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let data_dir = self.paths.root.display().to_string();
        let update_channel = self.settings.update_channel;
        let update_busy = self.update_busy;
        let update_label = self.update_action_label();
        let algorithm = self.settings.compact_algorithm;
        let allow_dstorage = self.settings.allow_dstorage_override;
        let include_xbox = self.settings.include_xbox_games;
        let custom_dirs = self.settings.custom_game_directories.clone();

        GroupBox::new().outline().child(
            v_flex()
                .gap_4()
                .child(settings_subgroup("Updates", false, cx))
                .child(settings_choice_row(
                    "Check for updates",
                    Some("Same check as the brand menu and About dialog."),
                    Button::new("settings-check-updates")
                        .outline()
                        .label(update_label)
                        .disabled(update_busy)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.begin_update_action(window, cx);
                        })),
                    cx,
                ))
                .child(settings_choice_row(
                    "Update channel",
                    Some(
                        "Stable follows GitHub /releases/latest. Nightly follows published vX.Y.Z-nightly.* pre-releases.",
                    ),
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("update-channel-stable")
                                .label(UpdateChannel::Stable.label())
                                .when(update_channel == UpdateChannel::Stable, |b| b.primary())
                                .when(update_channel != UpdateChannel::Stable, |b| b.outline())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.set_update_channel(UpdateChannel::Stable, window, cx);
                                })),
                        )
                        .child(
                            Button::new("update-channel-nightly")
                                .label(UpdateChannel::Nightly.label())
                                .when(update_channel == UpdateChannel::Nightly, |b| b.primary())
                                .when(update_channel != UpdateChannel::Nightly, |b| b.outline())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.set_update_channel(UpdateChannel::Nightly, window, cx);
                                })),
                        ),
                    cx,
                ))
                .child(settings_subgroup("Compact", true, cx))
                .child(settings_choice_row(
                    "WOF algorithm",
                    Some("Always compact /EXE. Default XPRESS8K. LZX is Shelf only and is not selectable here."),
                    h_flex().gap_2().children(CompactAlgorithm::LIVE.into_iter().map(|algo| {
                        let selected = algorithm == algo;
                        Button::new(SharedString::from(format!("algo-{}", algo.label())))
                            .label(algo.label())
                            .small()
                            .when(selected, |b| b.primary())
                            .when(!selected, |b| b.outline())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.settings.compact_algorithm = algo;
                                cx.notify();
                            }))
                    })),
                    cx,
                ))
                .child(settings_choice_row(
                    "Allow DirectStorage override",
                    Some("If dstorage.dll or dstoragecore.dll is in the tree, compact is blocked unless this is On."),
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("dstorage-off")
                                .label("Off")
                                .when(!allow_dstorage, |b| b.primary())
                                .when(allow_dstorage, |b| b.outline())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.settings.allow_dstorage_override = false;
                                    this.live.set_allow_dstorage(false);
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("dstorage-on")
                                .label("On")
                                .when(allow_dstorage, |b| b.primary())
                                .when(!allow_dstorage, |b| b.outline())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.settings.allow_dstorage_override = true;
                                    this.live.set_allow_dstorage(true);
                                    cx.notify();
                                })),
                        ),
                    cx,
                ))
                .child(settings_choice_row(
                    "Include XboxGames / GDK",
                    Some("Off by default. Opt-in only. Never scans WindowsApps."),
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("xbox-off")
                                .label("Off")
                                .when(!include_xbox, |b| b.primary())
                                .when(include_xbox, |b| b.outline())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.set_include_xbox_games(false, cx);
                                })),
                        )
                        .child(
                            Button::new("xbox-on")
                                .label("On")
                                .when(include_xbox, |b| b.primary())
                                .when(!include_xbox, |b| b.outline())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.set_include_xbox_games(true, cx);
                                })),
                        ),
                    cx,
                ))
                .child(settings_subgroup("Custom games", true, cx))
                .child(settings_choice_row(
                    "Game folders",
                    Some("Folders that launchers do not list. Each folder is one title. Saved immediately."),
                    Button::new("add-custom-game-dir")
                        .outline()
                        .icon(Icon::empty().path("icons/plus.svg"))
                        .label("Add folder")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.prompt_add_custom_game_directory(window, cx);
                        })),
                    cx,
                ))
                .children(custom_dirs.into_iter().enumerate().map(|(i, path)| {
                    let display = path.display().to_string();
                    let short = shorten_path_display(&display);
                    let open_path = path.clone();
                    let remove_path = path.clone();
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_x_hidden()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(short),
                        )
                        .child(
                            Button::new(SharedString::from(format!("open-custom-dir-{i}")))
                                .outline()
                                .small()
                                .icon(IconName::FolderOpen)
                                .label("Open")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Err(msg) = reveal_path(&open_path) {
                                        this.show_toast(msg, cx);
                                    }
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("remove-custom-dir-{i}")))
                                .outline()
                                .small()
                                .icon(Icon::empty().path("icons/delete.svg"))
                                .label("Remove")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.remove_custom_game_directory(remove_path.clone(), window, cx);
                                })),
                        )
                        .into_any_element()
                }))
                .child(settings_subgroup("App data", true, cx))
                .child(
                    v_flex()
                        .gap_1p5()
                        .child(settings_field_label("App data directory", cx))
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_x_hidden()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(data_dir.clone()),
                                )
                                .child(
                                    Clipboard::new("copy-data-dir")
                                        .value(SharedString::from(data_dir)),
                                )
                                .child(
                                    Button::new("open-data-dir")
                                        .outline()
                                        .small()
                                        .icon(IconName::FolderOpen)
                                        .label("Open")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Err(msg) = reveal_path(&this.paths.root) {
                                                this.show_toast(msg, cx);
                                            }
                                        })),
                                ),
                        )
                        .child(field_hint("settings.json and state.json live here.", cx)),
                ),
        )
    }
}

fn reveal_path(path: &std::path::Path) -> Result<(), String> {
    let _ = std::fs::create_dir_all(path);
    open::that(path).map_err(|e| format!("Could not open folder: {e}"))
}
