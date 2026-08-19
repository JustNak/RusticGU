use std::path::Path;
use std::time::SystemTime;

use crate::denylist::DenyList;
use crate::thresholds::{Recency, ShelfConfig};

/// Recommended NTFS/WOF algorithm (or an exclusion).
///
/// Maps to `compact.exe` / WOF:
/// - [`CompactPolicy::Lzx`]: `/EXE:LZX` (cold shelf)
/// - [`CompactPolicy::Xpress8k`]: `/EXE:XPRESS8K` (recently played)
/// - [`CompactPolicy::Xpress`]: `/EXE:XPRESS` walk-back when launching a
///   previously LZX-shelved title (lighter, faster to page in)
/// - [`CompactPolicy::Exclude`]: do not recommend any compact
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactPolicy {
    Lzx,
    Xpress8k,
    Xpress,
    Exclude { reason: String },
}

impl CompactPolicy {
    pub fn as_compact_exe_algo(&self) -> Option<&'static str> {
        match self {
            Self::Lzx => Some("LZX"),
            Self::Xpress8k => Some("XPRESS8K"),
            Self::Xpress => Some("XPRESS"),
            Self::Exclude { .. } => None,
        }
    }
}

/// Inputs for a single policy decision.
#[derive(Debug, Clone)]
pub struct PolicyInput<'a> {
    pub title: &'a str,
    pub last_played: Option<SystemTime>,
    pub is_launching: bool,
    pub store_id: Option<&'a str>,
    pub launcher_id: Option<&'a str>,
    pub install_folder: Option<&'a Path>,
    /// True when the title is currently stored as LZX (shelved).
    pub currently_shelved_lzx: bool,
}

impl<'a> PolicyInput<'a> {
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            last_played: None,
            is_launching: false,
            store_id: None,
            launcher_id: None,
            install_folder: None,
            currently_shelved_lzx: false,
        }
    }
}

/// Recommend a compact policy. Never unwraps.
///
/// **Unknown last-played (`None`) is conservative cold / LZX**, not a
/// fabricated recency. Only Steam `LastPlayed` and itch `localLastRunAt`
/// may fill `last_played`. Other stores stay `None` and therefore LZX
/// (unless excluded or launching an LZX-shelved title, which walks back
/// to XPRESS). Never invent a timestamp from mtime / INSTALLDATE.
pub fn recommend(
    input: &PolicyInput<'_>,
    now: SystemTime,
    config: &ShelfConfig,
    denylist: &DenyList,
) -> CompactPolicy {
    if let Some(rule) = denylist.match_title(
        input.title,
        input.store_id,
        input.launcher_id,
        input.install_folder,
    ) {
        return CompactPolicy::Exclude {
            reason: rule.reason.clone(),
        };
    }

    let age = input.last_played.and_then(|t| now.duration_since(t).ok());
    let recency = Recency::classify(age, config);
    let would_shelf = matches!(recency, Recency::Cold);

    if input.is_launching && (input.currently_shelved_lzx || would_shelf) {
        return CompactPolicy::Xpress;
    }

    match recency {
        Recency::Recent | Recency::Warm => CompactPolicy::Xpress8k,
        Recency::Cold => CompactPolicy::Lzx,
    }
}

/// Convenience using [`ShelfConfig::default`] and [`crate::default_denylist`].
pub fn recommend_default(input: &PolicyInput<'_>, now: SystemTime) -> CompactPolicy {
    recommend(
        input,
        now,
        &ShelfConfig::default(),
        &crate::default_denylist(),
    )
}
