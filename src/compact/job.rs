//! Elevated native-WOF helper (`rusticgu --wof-job <json>`).
//!
//! The GUI process writes a job listing explicit skip-filtered paths, then
//! relaunches itself with `runas` so UAC happens only after ACCESS_DENIED.
//! The worker never walks arbitrary trees: every path must sit under `root`.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::settings::CompactAlgorithm;

use super::command::CompactOp;
use super::engine::{is_windows_apps_path, volume_filesystem, CompactResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobOp {
    Compress,
    Uncompress,
}

impl From<CompactOp> for JobOp {
    fn from(op: CompactOp) -> Self {
        match op {
            CompactOp::Compress => Self::Compress,
            CompactOp::Uncompress => Self::Uncompress,
        }
    }
}

impl From<JobOp> for CompactOp {
    fn from(op: JobOp) -> Self {
        match op {
            JobOp::Compress => CompactOp::Compress,
            JobOp::Uncompress => CompactOp::Uncompress,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WofJob {
    pub op: JobOp,
    pub algorithm: CompactAlgorithm,
    #[serde(default)]
    pub force: bool,
    pub root: PathBuf,
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WofJobResult {
    pub ok: usize,
    pub failed: usize,
    pub skipped: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

pub fn result_path_for_job(job_path: &Path) -> PathBuf {
    let mut out = job_path.as_os_str().to_os_string();
    out.push(".result.json");
    PathBuf::from(out)
}

pub fn path_is_under_root(path: &Path, root: &Path) -> bool {
    let Some(path_comps) = resolved_components(path) else {
        return false;
    };
    let Some(root_comps) = resolved_components(root) else {
        return false;
    };
    components_under(&path_comps, &root_comps)
}

/// Resolve `.` / `..` lexically. When a prefix of the path exists, canonicalize
/// it so junctions and symlinks that leave `root` are rejected.
fn resolved_components(path: &Path) -> Option<Vec<String>> {
    let mut current = path.to_path_buf();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(canon) = std::fs::canonicalize(&current) {
            let mut comps = lexical_components(&strip_verbatim(&canon))?;
            tail.reverse();
            for part in tail {
                let name = part.to_string_lossy();
                match name.as_ref() {
                    "." => {}
                    ".." => {
                        if !pop_normal(&mut comps) {
                            return None;
                        }
                    }
                    _ => comps.push(name.to_ascii_lowercase()),
                }
            }
            return Some(comps);
        }
        match (
            current.file_name().map(|n| n.to_os_string()),
            current.parent(),
        ) {
            (Some(name), Some(parent)) if parent.as_os_str() != current.as_os_str() => {
                tail.push(name);
                current = parent.to_path_buf();
            }
            _ => return lexical_components(path),
        }
    }
}

fn strip_verbatim(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        if let Some(unc) = rest.strip_prefix("UNC\\") {
            return PathBuf::from(format!(r"\\{unc}"));
        }
        return PathBuf::from(rest);
    }
    if let Some(rest) = s.strip_prefix("//?/") {
        if let Some(unc) = rest.strip_prefix("UNC/") {
            return PathBuf::from(format!("//{unc}"));
        }
        return PathBuf::from(rest);
    }
    path.to_path_buf()
}

fn lexical_components(path: &Path) -> Option<Vec<String>> {
    let unified = path.to_string_lossy().replace('\\', "/");
    let path = Path::new(&unified);
    let mut out: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => {
                out.push(
                    prefix
                        .as_os_str()
                        .to_string_lossy()
                        .replace('\\', "/")
                        .to_ascii_lowercase(),
                );
            }
            Component::RootDir => out.push("/".into()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !pop_normal(&mut out) {
                    return None;
                }
            }
            Component::Normal(name) => {
                out.push(name.to_string_lossy().to_ascii_lowercase());
            }
        }
    }
    Some(out)
}

fn pop_normal(comps: &mut Vec<String>) -> bool {
    match comps.last().map(String::as_str) {
        None | Some("/") => false,
        Some(s) if s.ends_with(':') => false,
        _ => {
            comps.pop();
            true
        }
    }
}

fn components_under(path: &[String], root: &[String]) -> bool {
    path.len() >= root.len() && path.iter().zip(root.iter()).all(|(a, b)| a == b)
}

pub fn validate_job(job: &WofJob) -> Result<(), String> {
    if is_windows_apps_path(&job.root) {
        return Err("WindowsApps folders cannot be compacted.".into());
    }
    if !job.root.exists() {
        return Err("The game folder is missing.".into());
    }
    if let Some(fs) = volume_filesystem(&job.root) {
        if !fs.eq_ignore_ascii_case("NTFS") {
            return Err(format!("WOF Compact /EXE requires NTFS (found {fs})."));
        }
    }
    for file in &job.files {
        if !path_is_under_root(file, &job.root) {
            return Err(format!(
                "WOF job path is outside the install root: {}",
                file.display()
            ));
        }
    }
    Ok(())
}

pub fn interpret_job_result(
    exit: u32,
    result: Option<&WofJobResult>,
    op: CompactOp,
) -> Result<CompactResult, String> {
    let verb = match op {
        CompactOp::Compress => "Compacted",
        CompactOp::Uncompress => "Uncompacted",
    };
    if let Some(r) = result {
        if r.failed > 0 && r.ok == 0 && r.skipped == 0 {
            return Err(r
                .last_error
                .clone()
                .unwrap_or_else(|| format!("Elevated WOF job failed (exit {exit}).")));
        }
        return Ok(CompactResult {
            ok: true,
            message: format!("{verb} with WOF /EXE."),
        });
    }
    if exit == 0 {
        Ok(CompactResult {
            ok: true,
            message: format!("{verb} with WOF /EXE."),
        })
    } else {
        Err(format!("Elevated WOF job failed (exit {exit})."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compact::command::CompactOp;

    #[test]
    fn paths_must_stay_under_root() {
        let root = Path::new(r"D:\SteamLibrary\steamapps\common\Foo");
        assert!(path_is_under_root(&root.join("bin").join("game.exe"), root));
        assert!(path_is_under_root(root, root));
        assert!(path_is_under_root(
            Path::new(r"D:\SteamLibrary\steamapps\common\Foo\bin\..\play.exe"),
            root
        ));
        assert!(!path_is_under_root(
            Path::new(r"D:\Windows\System32\cmd.exe"),
            root
        ));
        assert!(!path_is_under_root(
            Path::new(r"D:\SteamLibrary\steamapps\common\FooExtra\x.exe"),
            root
        ));
        assert!(
            !path_is_under_root(
                Path::new(r"D:\SteamLibrary\steamapps\common\Foo\..\Bar\x.exe"),
                root
            ),
            "lexical .. must not escape the install root"
        );
        assert!(!path_is_under_root(
            &root.join("..").join("Bar").join("x.exe"),
            root
        ));
    }

    #[test]
    fn symlink_or_junction_escape_is_rejected() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp = std::env::temp_dir().join(format!(
            "rusticgu-job-symlink-{}-{}",
            std::process::id(),
            stamp
        ));
        let root = tmp.join("root");
        let outside = tmp.join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let secret = outside.join("secret.bin");
        std::fs::write(&secret, b"x").unwrap();
        let link = root.join("escape");
        let linked = {
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(&outside, &link).is_ok()
            }
            #[cfg(windows)]
            {
                std::os::windows::fs::symlink_dir(&outside, &link).is_ok()
            }
            #[cfg(not(any(unix, windows)))]
            {
                false
            }
        };
        if linked {
            assert!(
                !path_is_under_root(&link.join("secret.bin"), &root),
                "reparse points that leave the install root must be rejected"
            );
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn interpret_uses_real_exit_and_result_json() {
        let ok = WofJobResult {
            ok: 3,
            skipped: 1,
            failed: 1,
            last_error: Some("one file failed".into()),
        };
        let result = interpret_job_result(0, Some(&ok), CompactOp::Compress).unwrap();
        assert!(result.ok);
        assert!(result.message.contains("Compacted"));

        let all_fail = WofJobResult {
            failed: 2,
            last_error: Some("Access is denied.".into()),
            ..WofJobResult::default()
        };
        let err = interpret_job_result(1, Some(&all_fail), CompactOp::Compress).unwrap_err();
        assert!(err.contains("Access is denied"), "{err}");

        let no_json = interpret_job_result(7, None, CompactOp::Uncompress).unwrap_err();
        assert!(no_json.contains("exit 7"), "{no_json}");

        let silent_ok = interpret_job_result(0, None, CompactOp::Uncompress).unwrap();
        assert!(silent_ok.message.contains("Uncompacted"));
    }

    #[test]
    fn job_json_roundtrip() {
        let job = WofJob {
            op: JobOp::Compress,
            algorithm: CompactAlgorithm::Xpress8k,
            force: true,
            root: PathBuf::from(r"C:\games\Foo"),
            files: vec![PathBuf::from(r"C:\games\Foo\play.exe")],
        };
        let raw = serde_json::to_string(&job).unwrap();
        let back: WofJob = serde_json::from_str(&raw).unwrap();
        assert!(back.force);
        assert_eq!(back.files.len(), 1);
        assert_eq!(CompactOp::from(back.op), CompactOp::Compress);
    }

    #[test]
    fn validate_refuses_windowsapps_and_escape() {
        let job = WofJob {
            op: JobOp::Compress,
            algorithm: CompactAlgorithm::Xpress8k,
            force: false,
            root: PathBuf::from(r"C:\Program Files\WindowsApps\Foo.Game_1.0"),
            files: vec![],
        };
        let err = validate_job(&job).unwrap_err();
        assert!(err.to_ascii_lowercase().contains("windowsapps"), "{err}");

        let tmp = std::env::temp_dir().join(format!(
            "rusticgu-job-root-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let escaped = WofJob {
            op: JobOp::Compress,
            algorithm: CompactAlgorithm::Xpress8k,
            force: false,
            root: tmp.clone(),
            files: vec![PathBuf::from("/etc/passwd")],
        };
        let err = validate_job(&escaped).unwrap_err();
        assert!(err.to_ascii_lowercase().contains("outside"), "{err}");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
