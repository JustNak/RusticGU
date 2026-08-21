use gpui::{
    div, prelude::FluentBuilder, px, Context, FontWeight, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled,
};
use gpui_component::{h_flex, ActiveTheme, Icon, Sizable, StyledExt};

use super::super::filter::FilterKind;
use super::super::settings_category::SettingsCategory;
use super::super::LibraryApp;

pub(crate) fn format_nav_count(count: i32) -> SharedString {
    if count > 999 {
        "999+".into()
    } else {
        count.to_string().into()
    }
}

pub(crate) fn nav_item(
    label: &'static str,
    filter: FilterKind,
    count: i32,
    active: bool,
    cx: &mut Context<LibraryApp>,
) -> impl IntoElement {
    nav_row(label, filter, count, active, false, cx)
}

pub(crate) fn store_nav_item(
    filter: FilterKind,
    count: i32,
    active: bool,
    cx: &mut Context<LibraryApp>,
) -> impl IntoElement {
    nav_row(filter.label(), filter, count, active, true, cx)
}

fn nav_row(
    label: &'static str,
    filter: FilterKind,
    count: i32,
    active: bool,
    nested: bool,
    cx: &mut Context<LibraryApp>,
) -> impl IntoElement {
    let theme = cx.theme().clone();
    let bg = if active {
        theme.list_active
    } else {
        theme.transparent
    };
    let fg = if active {
        theme.sidebar_accent_foreground
    } else {
        theme.sidebar_foreground
    };
    let icon_color = if active {
        theme.sidebar_primary
    } else {
        theme.muted_foreground
    };
    let count_color = if active {
        theme.sidebar_primary
    } else {
        theme.muted_foreground
    };
    let row_id = if nested {
        SharedString::from(format!("nav-store-{label}"))
    } else {
        SharedString::from(format!("nav-{label}"))
    };

    h_flex()
        .id(row_id)
        .relative()
        .h(px(if nested { 32. } else { 38. }))
        .px_2()
        .pl(px(if nested { 22. } else { 8. }))
        .gap_2()
        .items_center()
        .rounded(theme.radius)
        .bg(bg)
        .hover(|s| {
            s.bg(if active {
                theme.list_active
            } else {
                theme.secondary.opacity(0.5)
            })
        })
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, window, cx| {
            this.select_filter(filter, window, cx);
        }))
        .when(active && !nested, |el| {
            el.child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(8.))
                    .bottom(px(8.))
                    .w(px(3.))
                    .rounded_full()
                    .bg(theme.primary),
            )
        })
        .when(active && nested, |el| {
            el.child(
                div()
                    .absolute()
                    .left(px(10.))
                    .top(px(8.))
                    .bottom(px(8.))
                    .w(px(2.))
                    .rounded_full()
                    .bg(theme.primary),
            )
        })
        .child(match filter.nav_icon_path() {
            Some(path) => Icon::empty()
                .path(path)
                .with_size(px(if nested { 14. } else { 16. }))
                .text_color(icon_color),
            None => Icon::new(filter.nav_icon())
                .with_size(px(if nested { 14. } else { 16. }))
                .text_color(icon_color),
        })
        .child(
            div()
                .flex_1()
                .text_sm()
                .font_weight(if active {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_color(fg)
                .child(label),
        )
        .when(count >= 0, |el| {
            el.child(
                div()
                    .text_xs()
                    .font_medium()
                    .text_color(count_color)
                    .child(format_nav_count(count)),
            )
        })
}

pub(crate) fn settings_nav_item(
    category: SettingsCategory,
    active: bool,
    cx: &mut Context<LibraryApp>,
) -> impl IntoElement {
    let theme = cx.theme().clone();
    let label = category.label();
    let bg = if active {
        theme
            .secondary
            .opacity(if theme.is_dark() { 0.55 } else { 0.85 })
    } else {
        theme.transparent
    };
    let fg = if active {
        theme.sidebar_accent_foreground
    } else {
        theme.sidebar_foreground
    };
    let icon_color = if active {
        theme.sidebar_primary
    } else {
        theme.muted_foreground
    };

    h_flex()
        .id(SharedString::from(format!("settings-nav-{label}")))
        .h(px(36.))
        .px_2()
        .gap_2()
        .items_center()
        .rounded(theme.radius)
        .bg(bg)
        .hover(|s| {
            s.bg(if active {
                theme
                    .secondary
                    .opacity(if theme.is_dark() { 0.65 } else { 0.95 })
            } else {
                theme.secondary.opacity(0.45)
            })
        })
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, _, cx| {
            if this.settings_category != category {
                this.settings_category = category;
                cx.notify();
            }
        }))
        .child(
            Icon::new(category.icon())
                .with_size(px(15.))
                .text_color(icon_color),
        )
        .child(
            div()
                .flex_1()
                .text_sm()
                .font_weight(if active {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_color(fg)
                .child(label),
        )
}

#[cfg(test)]
mod tests {
    use super::format_nav_count;

    #[test]
    fn nav_count_shows_exact_value_up_to_999() {
        assert_eq!(format_nav_count(0).as_ref(), "0");
        assert_eq!(format_nav_count(42).as_ref(), "42");
        assert_eq!(format_nav_count(999).as_ref(), "999");
    }

    #[test]
    fn nav_count_caps_above_999() {
        assert_eq!(format_nav_count(1000).as_ref(), "999+");
    }
}
