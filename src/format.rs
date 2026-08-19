use std::time::{SystemTime, UNIX_EPOCH};

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else if value >= 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

/// Format a pair of logical vs on-disk sizes for game cards.
pub fn format_size_pair(logical: Option<u64>, on_disk: Option<u64>) -> String {
    match (logical, on_disk) {
        (Some(logical), Some(on_disk)) if on_disk < logical => {
            format!(
                "{} on disk · {} logical",
                format_bytes(on_disk),
                format_bytes(logical)
            )
        }
        (Some(logical), Some(_)) => format_bytes(logical),
        (Some(logical), None) => format_bytes(logical),
        (None, Some(on_disk)) => format!("{} on disk", format_bytes(on_disk)),
        (None, None) => "Size unknown".into(),
    }
}

pub fn format_date(unix_secs: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let age = now.saturating_sub(unix_secs);
    if age < 60 {
        "Just now".into()
    } else if age < 3600 {
        format!("{}m ago", age / 60)
    } else if age < 86_400 {
        format!("{}h ago", age / 3600)
    } else {
        format_absolute_date_fallback(unix_secs)
    }
}

fn format_absolute_date_fallback(unix_secs: u64) -> String {
    let z = (unix_secs / 86_400) as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{m:02}/{d:02}/{y:04}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_scales() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(10 * 1024 * 1024), "10.0 MB");
    }

    #[test]
    fn size_pair_shows_savings() {
        let text = format_size_pair(Some(10 * 1024 * 1024), Some(4 * 1024 * 1024));
        assert!(text.contains("on disk"));
        assert!(text.contains("logical"));
    }
}
