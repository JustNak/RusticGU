//! Settings mini-nav category enum (shared by shell state and nav widgets).

use gpui_component::IconName;

/// Settings mini-nav categories. Switching does not discard the draft.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SettingsCategory {
    #[default]
    General,
    System,
    Appearance,
}

impl SettingsCategory {
    pub(crate) const ALL: [Self; 3] = [Self::General, Self::System, Self::Appearance];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::System => "System",
            Self::Appearance => "Appearance",
        }
    }

    pub(crate) fn icon(self) -> IconName {
        match self {
            Self::General => IconName::Folder,
            Self::System => IconName::Settings,
            Self::Appearance => IconName::Palette,
        }
    }

    pub(crate) fn panel_title(self) -> &'static str {
        self.label()
    }
}
