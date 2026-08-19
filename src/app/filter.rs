//! Main-window view filter (library vs settings).

use gpui_component::IconName;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilterKind {
    #[default]
    Library,
    Compacted,
    Uncompacted,
    Settings,
}

impl FilterKind {
    pub fn nav_icon(self) -> IconName {
        match self {
            Self::Library => IconName::Inbox,
            Self::Compacted => IconName::Folder,
            Self::Uncompacted => IconName::FolderOpen,
            Self::Settings => IconName::Settings,
        }
    }

    /// Asset path when the glyph is not a stock [`IconName`].
    pub fn nav_icon_path(self) -> Option<&'static str> {
        match self {
            Self::Library => Some("icons/gamepad.svg"),
            Self::Compacted => Some("icons/file-archive.svg"),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Library => "Library",
            Self::Compacted => "Compacted",
            Self::Uncompacted => "Uncompacted",
            Self::Settings => "Settings",
        }
    }
}
