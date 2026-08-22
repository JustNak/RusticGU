//! Shared UI widget helpers.

mod chrome;
mod nav;
mod path;
mod progress;
mod settings;

pub(crate) use chrome::render_vignette_overlay;
pub(crate) use nav::{nav_item, settings_nav_item, store_nav_item};
pub(crate) use path::{prompt_custom_game_directory, shorten_path_display};
pub(crate) use progress::styled_progress;
pub(crate) use settings::{
    accent_custom_swatch, accent_hsl_slider_row, accent_preset_swatch, field_hint,
    settings_choice_row, settings_field_label, settings_subgroup,
};
