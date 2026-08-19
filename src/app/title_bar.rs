//! Title bar chrome.

use gpui::{
    div, img, prelude::FluentBuilder, px, Context, InteractiveElement, IntoElement, ObjectFit,
    ParentElement, Styled, StyledImage,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::Input,
    menu::{DropdownMenu, PopupMenuItem},
    ActiveTheme, Icon, IconName, Sizable, StyledExt, TitleBar,
};

use super::filter::FilterKind;
use super::LibraryApp;
use crate::branding::{APP_LOGO_DARK, APP_LOGO_LIGHT, APP_NAME, APP_VERSION};
use crate::updater::{open_release_page, open_url};

impl LibraryApp {
    pub(crate) fn render_title_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let show_library_chrome = self.filter != FilterKind::Settings;
        let update_busy = self.update_busy;
        let update_action_label = self.update_action_label();
        let release_page_url = self
            .available_update
            .as_ref()
            .map(|info| info.html_url.clone());
        let view = cx.entity();

        let logo = if theme.is_dark() {
            APP_LOGO_DARK
        } else {
            APP_LOGO_LIGHT
        };
        #[cfg(target_os = "macos")]
        const TITLE_BAR_LEFT_PAD: f32 = 80.0;
        #[cfg(not(target_os = "macos"))]
        const TITLE_BAR_LEFT_PAD: f32 = 12.0;
        let brand_col_w = if self.filter == FilterKind::Settings {
            (self.settings.ui_density.sidebar_w() - TITLE_BAR_LEFT_PAD).max(80.0)
        } else {
            148.0
        };
        let brand_menu = {
            let view = view.clone();
            move |menu: gpui_component::menu::PopupMenu,
                  _window: &mut gpui::Window,
                  _menu_cx: &mut gpui::Context<gpui_component::menu::PopupMenu>| {
                let view = view.clone();
                menu.min_w(px(200.))
                    .item(
                        PopupMenuItem::new(if update_busy {
                            "Updating…".to_string()
                        } else {
                            update_action_label.clone()
                        })
                        .icon(Icon::empty().path("icons/rotate-cw.svg"))
                        .disabled(update_busy)
                        .on_click({
                            let view = view.clone();
                            move |_, window, cx| {
                                view.update(cx, |app, cx| {
                                    app.begin_update_action(window, cx);
                                });
                            }
                        }),
                    )
                    .separator()
                    .item(
                        PopupMenuItem::new("Open releases on GitHub")
                            .icon(IconName::ExternalLink)
                            .on_click({
                                let release_page_url = release_page_url.clone();
                                move |_, _, _| {
                                    if let Some(url) = &release_page_url {
                                        let _ = open_url(url);
                                    } else {
                                        let _ = open_release_page();
                                    }
                                }
                            }),
                    )
                    .separator()
                    .item(
                        PopupMenuItem::new(format!("About {APP_NAME}…  v{APP_VERSION}"))
                            .icon(IconName::Info)
                            .on_click({
                                let view = view.clone();
                                move |_, window, cx| {
                                    view.update(cx, |app, cx| {
                                        app.open_about_dialog(window, cx);
                                    });
                                }
                            }),
                    )
                    .separator()
                    .item(
                        PopupMenuItem::new("Exit")
                            .icon(IconName::WindowClose)
                            .on_click({
                                let view = view.clone();
                                move |_, _, cx| {
                                    view.update(cx, |app, cx| {
                                        app.force_quit_app(cx);
                                    });
                                }
                            }),
                    )
            }
        };

        TitleBar::new().h(px(48.)).child(
            h_flex()
                .id("title-bar-content")
                .w_full()
                .h_full()
                .items_center()
                .gap_2()
                .pr_1()
                .child(
                    h_flex()
                        .id("title-bar-brand")
                        .w(px(brand_col_w))
                        .h_full()
                        .flex_shrink_0()
                        .items_center()
                        .overflow_hidden()
                        .gap_2()
                        .child(
                            Button::new("app-brand-menu")
                                .ghost()
                                .compact()
                                .tooltip("App menu")
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .items_center()
                                        .child(
                                            img(logo)
                                                .w(px(22.))
                                                .h(px(22.))
                                                .object_fit(ObjectFit::Contain),
                                        )
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_semibold()
                                                .text_color(theme.foreground)
                                                .child(APP_NAME),
                                        ),
                                )
                                .dropdown_menu(brand_menu),
                        ),
                )
                .when(show_library_chrome, |el| {
                    el.child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .gap_2()
                            .child(Input::new(&self.search_input).w_full().small())
                            .child(
                                Button::new("title-refresh-library")
                                    .ghost()
                                    .icon(Icon::empty().path("icons/rotate-cw.svg"))
                                    .tooltip("Rescan library")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.refresh_library(cx);
                                    })),
                            )
                            .child(
                                Button::new("title-open-settings")
                                    .ghost()
                                    .icon(IconName::Settings)
                                    .tooltip("Settings")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.select_filter(FilterKind::Settings, window, cx);
                                    })),
                            ),
                    )
                })
                .when(!show_library_chrome, |el| el.child(div().flex_1())),
        )
    }
}
