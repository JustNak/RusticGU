//! Minimal Valve KeyValues (VDF / ACF) parser.
//!
//! Enough to read `libraryfolders.vdf` and `appmanifest_*.acf`. Quoted keys and
//! values, nested objects, and `//` comments are supported.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VdfValue {
    String(String),
    Object(VdfObject),
}

pub type VdfObject = BTreeMap<String, VdfValue>;

impl VdfValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            Self::Object(_) => None,
        }
    }

    pub fn as_object(&self) -> Option<&VdfObject> {
        match self {
            Self::Object(map) => Some(map),
            Self::String(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub offset: usize,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at {}", self.message, self.offset)
    }
}

impl std::error::Error for ParseError {}

/// Parse a VDF / ACF document into a single top-level object.
///
/// Steam files typically wrap the payload in one named root (`"libraryfolders"`
/// / `"AppState"`). That root name is preserved as the sole key.
pub fn parse_vdf(input: &str) -> Result<VdfObject, ParseError> {
    let mut p = Parser::new(input);
    p.skip_ws_and_comments();
    if p.eof() {
        return Ok(VdfObject::new());
    }
    let mut root = VdfObject::new();
    while !p.eof() {
        p.skip_ws_and_comments();
        if p.eof() {
            break;
        }
        let key = p.parse_string()?;
        p.skip_ws_and_comments();
        let value = p.parse_value()?;
        root.insert(key, value);
        p.skip_ws_and_comments();
    }
    Ok(root)
}

/// Look up a string by walking slash-separated keys (case-insensitive).
pub fn lookup_str<'a>(obj: &'a VdfObject, path: &str) -> Option<&'a str> {
    let mut current = obj;
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    for (i, part) in parts.iter().enumerate() {
        let value = get_ci(current, part)?;
        if i + 1 == parts.len() {
            return value.as_str();
        }
        current = value.as_object()?;
    }
    None
}

/// Child object by case-insensitive key.
pub fn lookup_object<'a>(obj: &'a VdfObject, key: &str) -> Option<&'a VdfObject> {
    get_ci(obj, key).and_then(VdfValue::as_object)
}

pub fn get_ci<'a>(obj: &'a VdfObject, key: &str) -> Option<&'a VdfValue> {
    obj.get(key).or_else(|| {
        obj.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    })
}

struct Parser<'a> {
    src: &'a str,
    i: usize,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, i: 0 }
    }

    fn eof(&self) -> bool {
        self.i >= self.src.len()
    }

    fn peek(&self) -> Option<char> {
        self.src[self.i..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.i += ch.len_utf8();
        Some(ch)
    }

    fn error(&self, message: impl Into<String>) -> ParseError {
        ParseError {
            message: message.into(),
            offset: self.i,
        }
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => {
                    self.bump();
                }
                Some('/') if self.src[self.i..].starts_with("//") => {
                    while let Some(c) = self.peek() {
                        self.bump();
                        if c == '\n' {
                            break;
                        }
                    }
                }
                Some('/') if self.src[self.i..].starts_with("/*") => {
                    self.i += 2;
                    if let Some(end) = self.src[self.i..].find("*/") {
                        self.i += end + 2;
                    } else {
                        self.i = self.src.len();
                    }
                }
                _ => return,
            }
        }
    }

    fn parse_string(&mut self) -> Result<String, ParseError> {
        self.skip_ws_and_comments();
        match self.peek() {
            Some('"') => self.parse_quoted(),
            Some(c) if is_bare_start(c) => self.parse_bare(),
            Some(c) => Err(self.error(format!("expected string, found {c:?}"))),
            None => Err(self.error("expected string, found end of input")),
        }
    }

    fn parse_quoted(&mut self) -> Result<String, ParseError> {
        self.bump(); // "
        let mut out = String::new();
        while let Some(c) = self.bump() {
            match c {
                '"' => return Ok(out),
                '\\' => match self.bump() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('r') => out.push('\r'),
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => return Err(self.error("unterminated escape")),
                },
                other => out.push(other),
            }
        }
        Err(self.error("unterminated quoted string"))
    }

    fn parse_bare(&mut self) -> Result<String, ParseError> {
        let start = self.i;
        while let Some(c) = self.peek() {
            if c.is_whitespace() || c == '{' || c == '}' || c == '"' {
                break;
            }
            self.bump();
        }
        Ok(self.src[start..self.i].to_string())
    }

    fn parse_value(&mut self) -> Result<VdfValue, ParseError> {
        self.skip_ws_and_comments();
        match self.peek() {
            Some('{') => Ok(VdfValue::Object(self.parse_object()?)),
            Some(_) => Ok(VdfValue::String(self.parse_string()?)),
            None => Err(self.error("expected value")),
        }
    }

    fn parse_object(&mut self) -> Result<VdfObject, ParseError> {
        self.bump(); // {
        let mut map = VdfObject::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                Some('}') => {
                    self.bump();
                    return Ok(map);
                }
                None => return Err(self.error("unterminated object")),
                Some(_) => {
                    let key = self.parse_string()?;
                    self.skip_ws_and_comments();
                    let value = self.parse_value()?;
                    map.insert(key, value);
                }
            }
        }
    }
}

fn is_bare_start(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_libraryfolders() {
        let src = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"label"		""
		"contentid"		"123"
		"apps"
		{
			"730"		"111"
			"570"		"222"
		}
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
	}
}
"#;
        let root = parse_vdf(src).unwrap();
        let folders = lookup_object(&root, "libraryfolders").unwrap();
        let zero = lookup_object(folders, "0").unwrap();
        assert_eq!(
            lookup_str(zero, "path"),
            Some(r"C:\Program Files (x86)\Steam")
        );
        let apps = lookup_object(zero, "apps").unwrap();
        assert_eq!(lookup_str(apps, "730"), Some("111"));
        assert_eq!(
            lookup_str(lookup_object(folders, "1").unwrap(), "path"),
            Some(r"D:\SteamLibrary")
        );
    }

    #[test]
    fn parses_appmanifest() {
        let src = r#"
"AppState"
{
	"appid"		"730"
	"Universe"		"1"
	"name"		"Counter-Strike 2"
	"StateFlags"		"4"
	"installdir"		"Counter-Strike Global Offensive"
	"SizeOnDisk"		"41234567890"
	"buildid"		"99"
}
"#;
        let root = parse_vdf(src).unwrap();
        let state = lookup_object(&root, "AppState").unwrap();
        assert_eq!(lookup_str(state, "appid"), Some("730"));
        assert_eq!(lookup_str(state, "name"), Some("Counter-Strike 2"));
        assert_eq!(
            lookup_str(state, "installdir"),
            Some("Counter-Strike Global Offensive")
        );
        assert_eq!(lookup_str(state, "SizeOnDisk"), Some("41234567890"));
    }

    #[test]
    fn comments_and_bare_keys() {
        let src = r#"
// header
AppState
{
	appid 570
	name "Dota 2"
}
"#;
        let root = parse_vdf(src).unwrap();
        let state = lookup_object(&root, "AppState").unwrap();
        assert_eq!(lookup_str(state, "appid"), Some("570"));
        assert_eq!(lookup_str(state, "name"), Some("Dota 2"));
    }
}
