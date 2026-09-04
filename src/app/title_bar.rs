//! Title bar chrome.

use gpui::{
    div, prelude::FluentBuilder, px, Context, InteractiveElement, IntoElement, ParentElement,
    Styled,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::Input,
    Icon, Sizable, StyledExt, TitleBar,
};

use super::filter::FilterKind;
use super::LibraryApp;

const SEARCH_W: f32 = 320.0;

impl LibraryApp {
    pub(crate) fn render_title_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let show_library_chrome = self.filter != FilterKind::Settings;

        TitleBar::new().h(px(48.)).border_b_0().child(
            h_flex()
                .id("title-bar-content")
                .w_full()
                .h_full()
                .items_center()
                .gap_2()
                .pl_1()
                .pr_1()
                .when(show_library_chrome, |el| {
                    el.child(Input::new(&self.search_input).w(px(SEARCH_W)).h_8())
                        .child(
                            Button::new("title-add-folder")
                                .outline()
                                .h_8()
                                .w_8()
                                .icon(Icon::empty().path("icons/plus.svg"))
                                .tooltip("Add folder")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.prompt_add_custom_game_directory(window, cx);
                                })),
                        )
                        .child(div().flex_1())
                        .child(
                            Button::new("title-refresh-library")
                                .ghost()
                                .icon(Icon::empty().path("icons/rotate-cw.svg"))
                                .tooltip("Refresh")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.refresh_library(cx);
                                })),
                        )
                })
                .when(!show_library_chrome, |el| el.child(div().flex_1())),
        )
    }
}
