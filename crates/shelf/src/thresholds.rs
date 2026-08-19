use std::time::Duration;

/// Games played within this window stay on the fast-to-page-in algorithm.
pub const DEFAULT_RECENT_WITHIN: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Games idle this long, **or with unknown last-played (`None`)**, are
/// treated as cold / LZX. `None` is conservative, not a fake timestamp.
pub const DEFAULT_COLD_AFTER: Duration = Duration::from_secs(21 * 24 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShelfConfig {
    pub recent_within: Duration,
    pub cold_after: Duration,
}

impl Default for ShelfConfig {
    fn default() -> Self {
        Self {
            recent_within: DEFAULT_RECENT_WITHIN,
            cold_after: DEFAULT_COLD_AFTER,
        }
    }
}

impl ShelfConfig {
    pub fn sane_defaults() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recency {
    Recent,
    /// Between recent and cold: keep XPRESS8K, do not shelf yet.
    Warm,
    Cold,
}

impl Recency {
    pub fn classify(age: Option<Duration>, cfg: &ShelfConfig) -> Self {
        match age {
            None => Self::Cold,
            Some(age) if age <= cfg.recent_within => Self::Recent,
            Some(age) if age >= cfg.cold_after => Self::Cold,
            Some(_) => Self::Warm,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_played_is_cold() {
        assert_eq!(
            Recency::classify(None, &ShelfConfig::default()),
            Recency::Cold
        );
    }

    #[test]
    fn week_old_is_recent() {
        assert_eq!(
            Recency::classify(
                Some(Duration::from_secs(3 * 86400)),
                &ShelfConfig::default()
            ),
            Recency::Recent
        );
    }

    #[test]
    fn month_old_is_cold() {
        assert_eq!(
            Recency::classify(
                Some(Duration::from_secs(30 * 86400)),
                &ShelfConfig::default()
            ),
            Recency::Cold
        );
    }
}
