use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::error::{StoreError, StoreResult};
use crate::util::{file_name_eq_ignore_case, looks_like_volume_root, normalize_path_key, path_contains_component};

/// Directory listing entry from an injected index filesystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

/// Read-only view of launcher indexes. Tests inject maps; Windows uses `StdFs`.
///
/// Hard rules enforced here:
/// - never list a volume root (`D:\`)
/// - never open `butler.db` (itch logins)
/// - never enter `WindowsApps` (no takeown)
pub trait IndexFs {
    fn exists(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
    fn read_to_string(&self, path: &Path) -> StoreResult<String>;
    fn list_dir(&self, path: &Path) -> StoreResult<Vec<DirEntry>>;
}

pub fn reject_forbidden(path: &Path) -> StoreResult<()> {
    if looks_like_volume_root(path) {
        return Err(StoreError::forbidden(
            path,
            "refusing to list or read a volume root (no disk walks)",
        ));
    }
    if file_name_eq_ignore_case(path, "butler.db") || path_contains_component(path, "butler.db") {
        return Err(StoreError::forbidden(
            path,
            "itch butler.db contains logins and must never be opened",
        ));
    }
    if path_contains_component(path, "WindowsApps") {
        return Err(StoreError::forbidden(
            path,
            "WindowsApps is forbidden (no takeown / package walks)",
        ));
    }
    Ok(())
}

/// Real OS filesystem. Safe for Linux tests that point at fixture trees.
#[derive(Debug, Default, Clone, Copy)]
pub struct StdFs;

impl IndexFs for StdFs {
    fn exists(&self, path: &Path) -> bool {
        if reject_forbidden(path).is_err() {
            return false;
        }
        path.exists()
    }

    fn is_dir(&self, path: &Path) -> bool {
        if reject_forbidden(path).is_err() {
            return false;
        }
        path.is_dir()
    }

    fn read_to_string(&self, path: &Path) -> StoreResult<String> {
        reject_forbidden(path)?;
        fs::read_to_string(path).map_err(|e| StoreError::io(path, e))
    }

    fn list_dir(&self, path: &Path) -> StoreResult<Vec<DirEntry>> {
        reject_forbidden(path)?;
        let rd = fs::read_dir(path).map_err(|e| StoreError::io(path, e))?;
        let mut out = Vec::new();
        for ent in rd {
            let ent = ent.map_err(|e| StoreError::io(path, e))?;
            let child = ent.path();
            if file_name_eq_ignore_case(&child, "butler.db")
                || path_contains_component(&child, "WindowsApps")
            {
                continue;
            }
            out.push(DirEntry {
                name: ent.file_name().to_string_lossy().into_owned(),
                path: child.clone(),
                is_dir: child.is_dir(),
            });
        }
        out.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
        Ok(out)
    }
}

/// In-memory index tree. Paths are matched after `\` → `/` normalization.
#[derive(Debug, Default, Clone)]
pub struct MemoryFs {
    files: BTreeMap<String, Vec<u8>>,
    dirs: BTreeMap<String, ()>,
}

impl MemoryFs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_dir(&mut self, path: impl AsRef<Path>) {
        let key = normalize_path_key(path.as_ref());
        self.dirs.insert(key.clone(), ());
        // ensure ancestors exist
        let mut acc = String::new();
        for part in key.split('/') {
            if part.is_empty() {
                continue;
            }
            if !acc.is_empty() {
                acc.push('/');
            }
            acc.push_str(part);
            self.dirs.insert(acc.clone(), ());
        }
    }

    pub fn add_file(&mut self, path: impl AsRef<Path>, contents: impl Into<Vec<u8>>) {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                self.add_dir(parent);
            }
        }
        self.files
            .insert(normalize_path_key(path), contents.into());
    }

    fn lookup_file(&self, path: &Path) -> Option<&[u8]> {
        self.files
            .get(&normalize_path_key(path))
            .map(|v| v.as_slice())
    }

    fn has_dir(&self, path: &Path) -> bool {
        let key = normalize_path_key(path);
        self.dirs.contains_key(&key)
            || self.files.keys().any(|k| k.starts_with(&format!("{key}/")))
    }
}

impl IndexFs for MemoryFs {
    fn exists(&self, path: &Path) -> bool {
        if reject_forbidden(path).is_err() {
            return false;
        }
        let key = normalize_path_key(path);
        self.files.contains_key(&key) || self.has_dir(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        if reject_forbidden(path).is_err() {
            return false;
        }
        self.has_dir(path) && !self.files.contains_key(&normalize_path_key(path))
    }

    fn read_to_string(&self, path: &Path) -> StoreResult<String> {
        reject_forbidden(path)?;
        match self.lookup_file(path) {
            Some(bytes) => String::from_utf8(bytes.to_vec())
                .map_err(|e| StoreError::parse(path, e.to_string())),
            None => Err(StoreError::io(
                path,
                io::Error::new(ErrorKind::NotFound, "memory fs: file not found"),
            )),
        }
    }

    fn list_dir(&self, path: &Path) -> StoreResult<Vec<DirEntry>> {
        reject_forbidden(path)?;
        if !self.has_dir(path) {
            return Err(StoreError::io(
                path,
                io::Error::new(ErrorKind::NotFound, "memory fs: dir not found"),
            ));
        }
        let prefix = {
            let k = normalize_path_key(path);
            if k.is_empty() {
                String::new()
            } else {
                format!("{k}/")
            }
        };
        let mut names: BTreeMap<String, bool> = BTreeMap::new();
        let mut consider = |key: &str, is_file: bool| {
            if let Some(rest) = key.strip_prefix(&prefix) {
                if let Some((name, tail)) = rest.split_once('/') {
                    if !name.is_empty() {
                        names.entry(name.to_string()).or_insert(true);
                        let _ = tail;
                    }
                } else if is_file && !rest.is_empty() {
                    names.entry(rest.to_string()).or_insert(false);
                } else if !is_file && !rest.is_empty() {
                    names.entry(rest.to_string()).or_insert(true);
                }
            }
        };
        for key in self.files.keys() {
            consider(key, true);
        }
        for key in self.dirs.keys() {
            if key == &normalize_path_key(path) {
                continue;
            }
            consider(key, false);
        }
        let mut out = Vec::new();
        for (name, is_dir) in names {
            let child = path.join(&name);
            if file_name_eq_ignore_case(&child, "butler.db")
                || path_contains_component(&child, "WindowsApps")
            {
                continue;
            }
            out.push(DirEntry {
                name,
                path: child,
                is_dir,
            });
        }
        Ok(out)
    }
}

/// Records every path the inner filesystem was asked to read or list.
/// Tests use this to prove `butler.db` was never opened.
#[derive(Debug, Clone)]
pub struct RecordingFs<F> {
    inner: F,
    opened: Rc<RefCell<Vec<PathBuf>>>,
}

impl<F> RecordingFs<F> {
    pub fn new(inner: F) -> Self {
        Self {
            inner,
            opened: Rc::new(RefCell::new(Vec::new())),
        }
    }

    pub fn opened_paths(&self) -> Vec<PathBuf> {
        self.opened.borrow().clone()
    }

    pub fn record_handle(&self) -> Rc<RefCell<Vec<PathBuf>>> {
        Rc::clone(&self.opened)
    }

    fn record(&self, path: &Path) {
        self.opened.borrow_mut().push(path.to_path_buf());
    }
}

impl<F: IndexFs> IndexFs for RecordingFs<F> {
    fn exists(&self, path: &Path) -> bool {
        self.inner.exists(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.inner.is_dir(path)
    }

    fn read_to_string(&self, path: &Path) -> StoreResult<String> {
        self.record(path);
        self.inner.read_to_string(path)
    }

    fn list_dir(&self, path: &Path) -> StoreResult<Vec<DirEntry>> {
        self.record(path);
        self.inner.list_dir(path)
    }
}

pub fn never_opened_butler_db(paths: &[PathBuf]) -> bool {
    !paths.iter().any(|p| file_name_eq_ignore_case(p, "butler.db"))
}

impl<T: IndexFs + ?Sized> IndexFs for &T {
    fn exists(&self, path: &Path) -> bool {
        (**self).exists(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        (**self).is_dir(path)
    }

    fn read_to_string(&self, path: &Path) -> StoreResult<String> {
        (**self).read_to_string(path)
    }

    fn list_dir(&self, path: &Path) -> StoreResult<Vec<DirEntry>> {
        (**self).list_dir(path)
    }
}
