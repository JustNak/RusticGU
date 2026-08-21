//! Sidebar navigation.

use gpui::{
    div, px, Context, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
    Styled,
};
use gpui_component::{
    divider::Divider, h_flex, v_flex, ActiveTheme, Icon, IconName, Sizable, StyledExt,
};

use super::filter::{store_nav_entries, FilterKind};
use super::widgets::{nav_item, settings_nav_item, store_nav_item};
use super::LibraryApp;

impl LibraryApp {
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
            .p_3()
            .gap_0p5()
            .child(
                div()
                    .px_2()
                    .pb_1()
                    .text_xs()
                    .font_semibold()
                    .text_color(theme.muted_foreground)
                    .child("LIBRARY"),
            )
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
            .child(Divider::horizontal().my_2())
            .child(
                div()
                    .px_2()
                    .pb_1()
                    .text_xs()
                    .font_semibold()
                    .text_color(theme.muted_foreground)
                    .child("APP"),
            )
            .child(nav_item(
                "Settings",
                FilterKind::Settings,
                -1,
                filter == FilterKind::Settings,
                cx,
            ))
            .child(
                h_flex()
                    .id("nav-about")
                    .h(px(36.))
                    .px_2()
                    .gap_2()
                    .items_center()
                    .rounded(theme.radius)
                    .hover(|s| s.bg(theme.secondary.opacity(0.55)))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_about_dialog(window, cx);
                    }))
                    .child(
                        Icon::empty()
                            .path("icons/info.svg")
                            .with_size(px(15.))
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .text_color(theme.sidebar_foreground)
                            .child("About"),
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
            )
    }
}
