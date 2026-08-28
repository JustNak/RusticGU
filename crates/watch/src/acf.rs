use std::collections::BTreeMap;
use std::path::Path;

use crate::error::{WatchError, WatchResult};

/// Minimal Valve KeyValues (VDF) parser for `appmanifest_*.acf`.
/// Handles the nested `"AppState" { ... }` shape Steam writes.
pub fn parse_vdf(text: &str) -> WatchResult<VdfObject> {
    let mut p = Parser {
        chars: text.chars().peekable(),
        path: Path::new("<memory>"),
    };
    p.skip_ws();
    p.parse_object_body_or_named()
}

pub fn parse_vdf_path(path: &Path, text: &str) -> WatchResult<VdfObject> {
    let mut p = Parser {
        chars: text.chars().peekable(),
        path,
    };
    p.skip_ws();
    p.parse_object_body_or_named()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VdfObject {
    pub values: BTreeMap<String, String>,
    pub children: BTreeMap<String, VdfObject>,
}

impl VdfObject {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }

    pub fn get_u32(&self, key: &str) -> Option<u32> {
        self.get(key).and_then(|s| s.parse().ok())
    }

    pub fn get_u64(&self, key: &str) -> Option<u64> {
        self.get(key).and_then(|s| s.parse().ok())
    }

    /// Steam wraps fields in `"AppState" { }`. Prefer that child when present.
    pub fn app_state(&self) -> &VdfObject {
        self.children
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("AppState"))
            .map(|(_, v)| v)
            .unwrap_or(self)
    }
}

struct Parser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    path: &'a Path,
}

impl<'a> Parser<'a> {
    fn parse_object_body_or_named(&mut self) -> WatchResult<VdfObject> {
        if self.peek() == Some('"') {
            let name = self.parse_string()?;
            self.skip_ws();
            if self.peek() == Some('{') {
                let child = self.parse_braced()?;
                let mut root = VdfObject {
                    values: BTreeMap::new(),
                    children: BTreeMap::new(),
                };
                root.children.insert(name, child);
                self.skip_ws();
                while self.peek() == Some('"') {
                    let k = self.parse_string()?;
                    self.skip_ws();
                    if self.peek() == Some('{') {
                        let c = self.parse_braced()?;
                        root.children.insert(k, c);
                    } else {
                        let v = self.parse_string()?;
                        root.values.insert(k, v);
                    }
                    self.skip_ws();
                }
                return Ok(root);
            }
            return Err(WatchError::parse(self.path, "expected '{' after root key"));
        }
        if self.peek() == Some('{') {
            return self.parse_braced();
        }
        self.parse_object_until(None)
    }

    fn parse_braced(&mut self) -> WatchResult<VdfObject> {
        if self.next() != Some('{') {
            return Err(WatchError::parse(self.path, "expected '{'"));
        }
        let obj = self.parse_object_until(Some('}'))?;
        if self.next() != Some('}') {
            return Err(WatchError::parse(self.path, "expected '}'"));
        }
        Ok(obj)
    }

    fn parse_object_until(&mut self, end: Option<char>) -> WatchResult<VdfObject> {
        let mut obj = VdfObject {
            values: BTreeMap::new(),
            children: BTreeMap::new(),
        };
        loop {
            self.skip_ws();
            match self.peek() {
                None => {
                    if end.is_some() {
                        return Err(WatchError::parse(self.path, "unclosed VDF object"));
                    }
                    break;
                }
                Some(c) if end == Some(c) => break,
                Some('"') => {
                    let key = self.parse_string()?;
                    self.skip_ws();
                    if self.peek() == Some('{') {
                        let child = self.parse_braced()?;
                        obj.children.insert(key, child);
                    } else if self.peek() == Some('"') {
                        let val = self.parse_string()?;
                        obj.values.insert(key, val);
                    } else {
                        return Err(WatchError::parse(
                            self.path,
                            format!("expected value or object after '{key}'"),
                        ));
                    }
                }
                Some(c) => {
                    return Err(WatchError::parse(
                        self.path,
                        format!("unexpected '{c}' in VDF"),
                    ));
                }
            }
        }
        Ok(obj)
    }

    fn parse_string(&mut self) -> WatchResult<String> {
        if self.next() != Some('"') {
            return Err(WatchError::parse(self.path, "expected string"));
        }
        let mut out = String::new();
        loop {
            match self.next() {
                None => return Err(WatchError::parse(self.path, "unterminated string")),
                Some('"') => break,
                Some('\\') => match self.next() {
                    Some(c) => out.push(c),
                    None => return Err(WatchError::parse(self.path, "unterminated escape")),
                },
                Some(c) => out.push(c),
            }
        }
        Ok(out)
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
    fn parses_appmanifest() {
        let text = r#"
"AppState"
{
	"appid"		"570"
	"name"		"Dota 2"
	"StateFlags"		"4"
	"installdir"		"dota 2 beta"
	"BytesToDownload"		"0"
	"BytesDownloaded"		"0"
}
"#;
        let v = parse_vdf(text).unwrap();
        let app = v.app_state();
        assert_eq!(app.get("appid"), Some("570"));
        assert_eq!(app.get_u32("StateFlags"), Some(4));
        assert_eq!(app.get("name"), Some("Dota 2"));
    }
}
