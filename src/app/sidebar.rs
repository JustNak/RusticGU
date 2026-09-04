//! Sidebar navigation.

use gpui::{
    div, img, px, Context, InteractiveElement, IntoElement, ObjectFit, ParentElement,
    StatefulInteractiveElement, Styled, StyledImage,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    divider::Divider,
    h_flex,
    menu::{DropdownMenu, PopupMenuItem},
    v_flex, ActiveTheme, Icon, IconName, Sizable, StyledExt,
};

use super::filter::{store_nav_entries, FilterKind};
use super::widgets::{nav_item, settings_nav_item, store_nav_item};
use super::LibraryApp;
use crate::branding::{APP_LOGO_DARK, APP_LOGO_LIGHT, APP_NAME, APP_VERSION};
use crate::updater::{open_release_page, open_url};

/// Matches the title bar so the brand row and search chrome share one horizon.
const SIDEBAR_BRAND_H: f32 = 48.0;

impl LibraryApp {
    fn render_sidebar_brand(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
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

        h_flex()
            .id("sidebar-brand")
            .w_full()
            .h(px(SIDEBAR_BRAND_H))
            .px_3()
            .flex_shrink_0()
            .items_center()
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
                                    .w(px(26.))
                                    .h(px(26.))
                                    .rounded(px(6.))
                                    .object_fit(ObjectFit::Cover),
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
            )
    }

    pub(crate) fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let filter = self.filter;
        let sidebar_w = self.settings.ui_density.sidebar_w();
        let (all, compacted, uncompacted) = self.library_counts();
        let stores = store_nav_entries(&self.games);

        v_flex()
            .w(px(sidebar_w))
            .flex_shrink_0()
            .h_full()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .child(self.render_sidebar_brand(cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .p_3()
                    .gap_0p5()
                    .child(nav_item(
                        FilterKind::Library.label(),
                        FilterKind::Library,
                        all,
                        filter == FilterKind::Library,
                        cx,
                    ))
                    .children(stores.into_iter().map(|(store, count)| {
                        let kind = FilterKind::Store(store);
                        store_nav_item(kind, count, filter == kind, cx)
                    }))
                    .child(nav_item(
                        FilterKind::Compacted.label(),
                        FilterKind::Compacted,
                        compacted,
                        filter == FilterKind::Compacted,
                        cx,
                    ))
                    .child(nav_item(
                        FilterKind::Uncompacted.label(),
                        FilterKind::Uncompacted,
                        uncompacted,
                        filter == FilterKind::Uncompacted,
                        cx,
                    ))
                    .child(div().flex_1())
                    .child(
                        h_flex()
                            .id("sidebar-footer")
                            .w_full()
                            .items_center()
                            .justify_between()
                            .child(
                                Button::new("nav-settings")
                                    .ghost()
                                    .icon(Icon::empty().path("icons/settings.svg"))
                                    .tooltip("Settings")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.select_filter(FilterKind::Settings, window, cx);
                                    })),
                            )
                            .child(
                                Button::new("nav-about")
                                    .ghost()
                                    .icon(Icon::empty().path("icons/info.svg"))
                                    .tooltip("About")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_about_dialog(window, cx);
                                    })),
                            ),
                    ),
            )
    }

    pub(crate) fn render_settings_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let sidebar_w = self.settings.ui_density.sidebar_w();
        let category = self.settings_category;

        v_flex()
            .id("settings-sidebar")
            .w(px(sidebar_w))
            .flex_shrink_0()
            .h_full()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .child(self.render_sidebar_brand(cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .p_3()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .id("settings-nav-back")
                            .h(px(36.))
                            .px_2()
                            .gap_2()
                            .items_center()
                            .rounded(theme.radius)
                            .hover(|s| s.bg(theme.secondary.opacity(0.55)))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.leave_settings(window, cx);
                            }))
                            .child(
                                Icon::new(IconName::ChevronLeft)
                                    .with_size(px(15.))
                                    .text_color(theme.muted_foreground),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.sidebar_foreground)
                                    .child("Back"),
                            ),
                    )
                    .child(Divider::horizontal().my_2())
                    .children(
                        super::settings_category::SettingsCategory::ALL
                            .into_iter()
                            .map(|cat| settings_nav_item(cat, category == cat, cx)),
                    ),
            )
    }
}
