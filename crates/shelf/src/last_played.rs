//! Strict last-played sources.
//!
//! **Only** these two signals are safe:
//!
//! 1. Steam: `userdata\{id}\config\localconfig.vdf` → `apps/{appid}/LastPlayed`
//!    (that key **only**).
//! 2. itch: butlerd `CaveStats.localLastRunAt`.
//!
//! ACF `LastUpdated` is last **patch**, not last play — [`last_played_from_acf`]
//! always returns `None`.
//!
//! Epic / GOG / Xbox / Battle.net / EA / Ubisoft / Riot have **no** safe
//! last-play signal. Do not invent recency from mtime, `INSTALLDATE`, or
//! similar. Those stores stay [`None`].
//!
//! Shelf policy: [`None`] is a documented conservative default — treat as
//! **cold / LZX-eligible**, never a fabricated timestamp.

use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

/// Stores that may legally produce a last-played timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastPlayedSource {
    SteamLocalconfig,
    ItchCaveStats,
}

/// Which (if any) last-played source is allowed for a store id.
/// Steam is not discovered by `stores`; it is listed here so shelf callers
/// know the only two legal origins.
pub fn safe_last_played_source(store: &str) -> Option<LastPlayedSource> {
    match store.trim().to_ascii_lowercase().as_str() {
        "steam" => Some(LastPlayedSource::SteamLocalconfig),
        "itch" => Some(LastPlayedSource::ItchCaveStats),
        _ => None,
    }
}

/// ACF `LastUpdated` is last patch. This never returns a play time.
pub fn last_played_from_acf(_acf_text: &str) -> Option<SystemTime> {
    let _ = _acf_text;
    None
}

/// `apps/{appid}/LastPlayed` from `localconfig.vdf` only.
/// Other keys in the same file (including anything named `LastUpdated`) are ignored.
pub fn last_played_from_steam_localconfig(vdf_text: &str, app_id: u32) -> Option<SystemTime> {
    let root = parse_vdf(vdf_text)?;
    let apps = find_apps(&root)?;
    let id = app_id.to_string();
    let block = apps
        .children
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(&id))
        .map(|(_, v)| v)?;
    let raw = block
        .values
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("LastPlayed"))
        .map(|(_, v)| v.as_str())?;
    unix_to_system(raw.parse().ok()?)
}

/// `CaveStats.localLastRunAt` only (unix seconds or RFC3339 `…Z`).
pub fn last_played_from_itch_local_last_run_at(cave_json: &str) -> Option<SystemTime> {
    let raw = extract_json_field(cave_json, "localLastRunAt")?;
    parse_instant(&raw)
}

fn unix_to_system(secs: u64) -> Option<SystemTime> {
    Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
}

fn parse_instant(raw: &str) -> Option<SystemTime> {
    let s = raw.trim().trim_matches('"');
    if let Ok(n) = s.parse::<u64>() {
        return unix_to_system(n);
    }
    parse_rfc3339_z(s)
}

fn parse_rfc3339_z(s: &str) -> Option<SystemTime> {
    let s = s.strip_suffix('Z').or_else(|| s.strip_suffix('z'))?;
    let (date, time) = s.split_once('T').or_else(|| s.split_once('t'))?;
    let mut d = date.split('-');
    let y: i32 = d.next()?.parse().ok()?;
    let m: u32 = d.next()?.parse().ok()?;
    let day: u32 = d.next()?.parse().ok()?;
    let mut t = time.split(':');
    let h: u32 = t.next()?.parse().ok()?;
    let min: u32 = t.next()?.parse().ok()?;
    let sec_raw = t.next()?.split('.').next()?;
    let sec: u32 = sec_raw.parse().ok()?;
    let days = days_from_civil(y, m, day)?;
    let secs = days * 86400 + u64::from(h) * 3600 + u64::from(min) * 60 + u64::from(sec);
    unix_to_system(secs)
}

fn days_from_civil(y: i32, m: u32, d: u32) -> Option<u64> {
    if !(1..=12).contains(&m) || d == 0 || d > 31 {
        return None;
    }
    // Howard Hinnant civil-from-days; Unix epoch is 1970-01-01.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let month_adj = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * month_adj + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let z = i64::from(era) * 146097 + i64::from(doe) - 719468;
    if z < 0 {
        return None;
    }
    Some(z as u64)
}

fn extract_json_field(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let idx = text.find(&needle)?;
    let rest = text[idx + needle.len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    if rest.starts_with('"') {
        let end = rest[1..].find('"')?;
        return Some(rest[1..1 + end].to_string());
    }
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '-')
        .unwrap_or(rest.len());
    let num = rest[..end].trim();
    if num.is_empty() {
        None
    } else {
        Some(num.to_string())
    }
}

#[derive(Default)]
struct Vdf {
    values: BTreeMap<String, String>,
    children: BTreeMap<String, Vdf>,
}

fn find_apps<'a>(root: &'a Vdf) -> Option<&'a Vdf> {
    if root
        .children
        .keys()
        .any(|k| k.eq_ignore_ascii_case("apps"))
    {
        return root
            .children
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("apps"))
            .map(|(_, v)| v);
    }
    for child in root.children.values() {
        if let Some(found) = find_apps(child) {
            return Some(found);
        }
    }
    None
}

fn parse_vdf(text: &str) -> Option<Vdf> {
    let mut p = VdfParser {
        chars: text.chars().peekable(),
    };
    p.skip_ws();
    p.parse_root()
}

struct VdfParser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl<'a> VdfParser<'a> {
    fn parse_root(&mut self) -> Option<Vdf> {
        if self.peek() == Some('"') {
            let _name = self.parse_string()?;
            self.skip_ws();
            return self.parse_braced();
        }
        if self.peek() == Some('{') {
            return self.parse_braced();
        }
        None
    }

    fn parse_braced(&mut self) -> Option<Vdf> {
        if self.next() != Some('{') {
            return None;
        }
        let mut obj = Vdf::default();
        loop {
            self.skip_ws();
            match self.peek() {
                None | Some('}') => {
                    let _ = self.next();
                    break;
                }
                Some('"') => {
                    let key = self.parse_string()?;
                    self.skip_ws();
                    if self.peek() == Some('{') {
                        obj.children.insert(key, self.parse_braced()?);
                    } else {
                        obj.values.insert(key, self.parse_string()?);
                    }
                }
                _ => return None,
            }
        }
        Some(obj)
    }

    fn parse_string(&mut self) -> Option<String> {
        if self.next() != Some('"') {
            return None;
        }
        let mut out = String::new();
        loop {
            match self.next()? {
                '"' => break,
                '\\' => out.push(self.next()?),
                c => out.push(c),
            }
        }
        Some(out)
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.next();
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }

    fn next(&mut self) -> Option<char> {
        self.chars.next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_steam_and_itch_are_safe() {
        assert_eq!(
            safe_last_played_source("steam"),
            Some(LastPlayedSource::SteamLocalconfig)
        );
        assert_eq!(
            safe_last_played_source("itch"),
            Some(LastPlayedSource::ItchCaveStats)
        );
        for store in ["epic", "gog", "xbox", "battlenet", "ea", "ubisoft", "riot"] {
            assert_eq!(safe_last_played_source(store), None, "{store}");
        }
    }

    #[test]
    fn acf_last_updated_is_ignored() {
        let acf = r#"
"AppState"
{
	"appid"		"570"
	"LastUpdated"		"1700000000"
	"StateFlags"		"4"
}
"#;
        assert_eq!(last_played_from_acf(acf), None);
        assert_eq!(last_played_from_steam_localconfig(acf, 570), None);
    }
}
