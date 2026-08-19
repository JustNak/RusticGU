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

    pub fn label(self) -> &'static str {
        match self {
            Self::Library => "Library",
            Self::Compacted => "Compacted",
            Self::Uncompacted => "Uncompacted",
            Self::Settings => "Settings",
        }
    }
}
