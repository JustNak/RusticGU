use std::collections::BTreeMap;

use crate::error::{StoreError, StoreResult};

/// Injected Windows registry view. Tests supply a map; the real hive is
/// `cfg(windows)` only.
pub trait RegistryHive {
    fn list_subkeys(&self, key: &str) -> StoreResult<Vec<String>>;
    fn string_value(&self, key: &str, name: &str) -> StoreResult<Option<String>>;
}

fn normalize_key(key: &str) -> String {
    key.replace('/', "\\")
        .trim_matches('\\')
        .to_ascii_lowercase()
}

/// In-memory hive. Keys are case-insensitive; values are too.
#[derive(Debug, Default, Clone)]
pub struct MemoryHive {
    /// key → (subkeys, values name→data)
    nodes: BTreeMap<String, HiveNode>,
}

#[derive(Debug, Default, Clone)]
struct HiveNode {
    subkeys: Vec<String>,
    values: BTreeMap<String, String>,
}

impl MemoryHive {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_value(&mut self, key: &str, name: &str, value: impl Into<String>) {
        let nk = normalize_key(key);
        self.ensure_ancestors(&nk);
        let node = self.nodes.entry(nk).or_default();
        node.values.insert(name.to_ascii_lowercase(), value.into());
    }

    fn ensure_ancestors(&mut self, key: &str) {
        let parts: Vec<&str> = key.split('\\').filter(|p| !p.is_empty()).collect();
        let mut acc = String::new();
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                let parent = acc.clone();
                let node = self.nodes.entry(parent).or_default();
                if !node.subkeys.iter().any(|s| s.eq_ignore_ascii_case(part)) {
                    node.subkeys.push((*part).to_string());
                }
                acc.push('\\');
            }
            acc.push_str(part);
            self.nodes.entry(acc.clone()).or_default();
        }
    }
}

impl RegistryHive for MemoryHive {
    fn list_subkeys(&self, key: &str) -> StoreResult<Vec<String>> {
        match self.nodes.get(&normalize_key(key)) {
            Some(node) => Ok(node.subkeys.clone()),
            None => Err(StoreError::registry(key, "not found")),
        }
    }

    fn string_value(&self, key: &str, name: &str) -> StoreResult<Option<String>> {
        match self.nodes.get(&normalize_key(key)) {
            Some(node) => Ok(node.values.get(&name.to_ascii_lowercase()).cloned()),
            None => Err(StoreError::registry(key, "not found")),
        }
    }
}

/// Empty hive used when no registry backend is available (Linux default).
#[derive(Debug, Default, Clone, Copy)]
pub struct EmptyHive;

impl RegistryHive for EmptyHive {
    fn list_subkeys(&self, key: &str) -> StoreResult<Vec<String>> {
        Err(StoreError::registry(key, "not found"))
    }

    fn string_value(&self, key: &str, name: &str) -> StoreResult<Option<String>> {
        let _ = name;
        Err(StoreError::registry(key, "not found"))
    }
}

#[cfg(windows)]
pub struct WindowsHive;

#[cfg(windows)]
impl RegistryHive for WindowsHive {
    fn list_subkeys(&self, key: &str) -> StoreResult<Vec<String>> {
        let (hive, sub) = split_hive(key)?;
        let opened = open_both_views(hive, sub).map_err(|e| StoreError::registry(key, e))?;
        if opened.is_empty() {
            return Err(StoreError::registry(key, "not found"));
        }
        let mut names = Vec::new();
        for k in opened {
            let count = k.enum_keys().filter_map(|r| r.ok()).collect::<Vec<_>>();
            names.extend(count);
        }
        names.sort();
        names.dedup();
        Ok(names)
    }

    fn string_value(&self, key: &str, name: &str) -> StoreResult<Option<String>> {
        let (hive, sub) = split_hive(key)?;
        let opened = open_both_views(hive, sub).map_err(|e| StoreError::registry(key, e))?;
        if opened.is_empty() {
            return Err(StoreError::registry(key, "not found"));
        }
        for k in opened {
            if let Ok(v) = k.get_value::<String, _>(name) {
                return Ok(Some(v));
            }
        }
        Ok(None)
    }
}

#[cfg(windows)]
fn split_hive(key: &str) -> StoreResult<(winreg::RegKey, String)> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let key = key.trim_matches('\\');
    let lower = key.to_ascii_lowercase();
    if let Some(rest) = lower
        .strip_prefix("hklm\\")
        .or_else(|| lower.strip_prefix("hkey_local_machine\\"))
    {
        // Preserve original casing of the remainder from `key`.
        let orig = key
            .split_once('\\')
            .map(|(_, r)| r.to_string())
            .unwrap_or_default();
        let _ = rest;
        Ok((RegKey::predef(HKEY_LOCAL_MACHINE), orig))
    } else {
        Ok((RegKey::predef(HKEY_LOCAL_MACHINE), key.to_string()))
    }
}

#[cfg(windows)]
fn open_both_views(hive: winreg::RegKey, sub: String) -> Result<Vec<winreg::RegKey>, String> {
    use winreg::enums::{KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
    let mut out = Vec::new();
    for flag in [
        KEY_READ | KEY_WOW64_32KEY,
        KEY_READ | KEY_WOW64_64KEY,
        KEY_READ,
    ] {
        if let Ok(k) = hive.open_subkey_with_flags(&sub, flag) {
            out.push(k);
        }
    }
    Ok(out)
}

impl<T: RegistryHive + ?Sized> RegistryHive for &T {
    fn list_subkeys(&self, key: &str) -> StoreResult<Vec<String>> {
        (**self).list_subkeys(key)
    }

    fn string_value(&self, key: &str, name: &str) -> StoreResult<Option<String>> {
        (**self).string_value(key, name)
    }
}

/// First successful string among several value names.
pub fn first_value(hive: &impl RegistryHive, key: &str, names: &[&str]) -> Option<String> {
    for name in names {
        if let Ok(Some(v)) = hive.string_value(key, name) {
            let t = v.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}
