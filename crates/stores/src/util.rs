use std::path::Path;

/// Percent-decode a query-string value (`%20`, `%3A`, `+` → space).
pub fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                if let Ok(v) =
                    u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
                {
                    out.push(v);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parse `a=b&c=d` (optional leading `?`) into key/value pairs. Keys are
/// compared case-insensitively by callers.
pub fn parse_query(input: &str) -> Vec<(String, String)> {
    let s = input.strip_prefix('?').unwrap_or(input).trim();
    if s.is_empty() {
        return Vec::new();
    }
    s.split('&')
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            Some((percent_decode(k), percent_decode(v)))
        })
        .collect()
}

pub fn query_value<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.as_str())
}

/// Minimal `key: value` YAML reader for Riot product settings and similar
/// launcher indexes. Ignores nested blocks; quoted scalars are unquoted.
pub fn parse_simple_yaml_map(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('-') {
            continue;
        }
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let key = k.trim();
        if key.is_empty() || key.contains(' ') && !key.contains('_') {
            // skip likely non-scalar keys; still allow product_install_full_path
        }
        let mut val = v.trim().to_string();
        if (val.starts_with('"') && val.ends_with('"') && val.len() >= 2)
            || (val.starts_with('\'') && val.ends_with('\'') && val.len() >= 2)
        {
            val = val[1..val.len() - 1].to_string();
        }
        if !key.is_empty() {
            out.push((key.to_string(), val));
        }
    }
    out
}

pub fn yaml_value<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.as_str())
        .filter(|v| !v.is_empty())
}

/// First non-empty XML element or attribute value for `name`.
pub fn extract_xml_value(text: &str, name: &str) -> Option<String> {
    let attr = format!("{name}=\"");
    if let Some(idx) = text.find(&attr) {
        let rest = &text[idx + attr.len()..];
        if let Some(end) = rest.find('"') {
            let v = rest[..end].trim();
            if !v.is_empty() {
                return Some(unescape_xml(v));
            }
        }
    }
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    if let Some(start) = text.find(&open) {
        let rest = &text[start + open.len()..];
        if let Some(end) = rest.find(&close) {
            let v = rest[..end].trim();
            if !v.is_empty() {
                return Some(unescape_xml(v));
            }
        }
    }
    None
}

fn unescape_xml(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

pub fn normalize_path_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string()
}

/// Volume-root check used as a hard guard against disk walks.
pub fn looks_like_volume_root(path: &Path) -> bool {
    if path == Path::new("/") {
        return true;
    }
    let s = path.to_string_lossy();
    let trimmed = s.trim_end_matches(['\\', '/']);
    // `D:` or `D:\`
    let bytes = trimmed.as_bytes();
    bytes.len() == 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic()
}

pub fn file_name_eq_ignore_case(path: &Path, name: &str) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case(name))
}

pub fn path_contains_component(path: &Path, name: &str) -> bool {
    path.components().any(|c| {
        c.as_os_str()
            .to_str()
            .is_some_and(|s| s.eq_ignore_ascii_case(name))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decode_windows_path() {
        assert_eq!(
            percent_decode("C%3A%5CGames%5CTitanfall%202"),
            "C:\\Games\\Titanfall 2"
        );
    }

    #[test]
    fn query_is_case_insensitive() {
        let pairs = parse_query("?id=Origin.OFR.1&dipInstallPath=C%3A%5CG&displayName=X");
        assert_eq!(query_value(&pairs, "DIPINSTALLPATH"), Some("C:\\G"));
        assert_eq!(query_value(&pairs, "id"), Some("Origin.OFR.1"));
    }

    #[test]
    fn volume_roots_detected() {
        assert!(looks_like_volume_root(Path::new("D:")));
        assert!(looks_like_volume_root(Path::new("D:\\")));
        assert!(looks_like_volume_root(Path::new("/")));
        assert!(!looks_like_volume_root(Path::new(
            "C:\\ProgramData\\Epic\\Manifests"
        )));
    }
}
