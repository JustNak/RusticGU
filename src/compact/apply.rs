//! Parallel native-WOF apply. `compact.exe` is only a last-resort fallback
//! when `WofUtil.dll` cannot be loaded.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::settings::CompactAlgorithm;

use super::command::CompactOp;
use super::engine::{preflight, running_exe_in_tree, CompactProgress, CompactResult};
#[cfg(all(windows, not(test)))]
use super::job::interpret_job_result;
use super::job::{result_path_for_job, validate_job, JobOp, WofJob, WofJobResult};
use super::skip::{collect_included_files, should_skip};
use super::wof::{
    collect_wof_backed_files, compress_file, detect, effective_cluster, looks_incompressible,
    same_wof_algorithm, too_small, uncompress_file, volume_seek_penalty, wof_runtime_available,
    worker_count, WofError, WofStatus,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileOutcome {
    Ok,
    Skipped,
    AccessDenied,
    Failed,
}

struct ApplyStats {
    ok: usize,
    skipped: usize,
    failed: usize,
    last_error: Option<String>,
    denied: Vec<PathBuf>,
}

impl ApplyStats {
    fn merge_job(&mut self, job: WofJobResult) {
        self.ok = self.ok.saturating_add(job.ok);
        self.skipped = self.skipped.saturating_add(job.skipped);
        self.failed = self.failed.saturating_add(job.failed);
        if job.last_error.is_some() {
            self.last_error = job.last_error;
        }
    }

    fn any_ok(&self) -> bool {
        self.ok > 0 || self.skipped > 0
    }
}

pub fn apply_wof(
    op: CompactOp,
    root: &Path,
    algorithm: CompactAlgorithm,
    allow_dstorage: bool,
    explicit_files: Option<&[PathBuf]>,
    force: bool,
    mut progress: impl FnMut(CompactProgress) + Send,
) -> Result<CompactResult, String> {
    preflight(root, allow_dstorage).map_err(|e| e.to_string())?;

    let files = match (op, explicit_files) {
        (_, Some(files)) => resolve_explicit(root, files),
        (CompactOp::Uncompress, None) => collect_wof_backed_files(root),
        (CompactOp::Compress, None) => collect_included_files(root),
    };

    let start_message = if force {
        "Changing compression…".into()
    } else {
        match op {
            CompactOp::Compress => "Starting WOF compact…".into(),
            CompactOp::Uncompress => "Starting WOF uncompact…".into(),
        }
    };
    progress(CompactProgress {
        processed: 0,
        total: files.len().max(1),
        message: start_message,
    });

    if files.is_empty() {
        return Ok(CompactResult {
            ok: true,
            message: match op {
                CompactOp::Compress => "Nothing to compact (skip list excluded every file).".into(),
                CompactOp::Uncompress => "Nothing to uncompact (no WOF-backed files).".into(),
            },
        });
    }

    if !wof_runtime_available() {
        return super::exe::apply_via_compact_exe(
            op,
            root,
            algorithm,
            explicit_files,
            force,
            progress,
        );
    }

    let cluster = effective_cluster(root);
    let workers = worker_count(algorithm, volume_seek_penalty(root));
    let mut stats = run_files_parallel(
        op,
        root,
        &files,
        algorithm,
        force,
        cluster,
        workers,
        &mut progress,
    );

    if !stats.denied.is_empty() {
        progress(CompactProgress {
            processed: (stats.ok + stats.skipped + stats.failed).min(files.len()),
            total: files.len().max(1),
            message: "Access denied. Retrying elevated…".into(),
        });
        let job = WofJob {
            op: JobOp::from(op),
            algorithm,
            force,
            root: root.to_path_buf(),
            files: stats.denied.split_off(0),
        };
        match elevate_and_run(&job) {
            Ok(result) => stats.merge_job(result),
            Err(err) => {
                stats.failed = stats.failed.saturating_add(job.files.len());
                if !stats.any_ok() {
                    return Err(err);
                }
                stats.last_error = Some(err);
            }
        }
    }

    progress(CompactProgress {
        processed: files.len().max(1),
        total: files.len().max(1),
        message: "Finished.".into(),
    });

    if stats.failed > 0 && !stats.any_ok() {
        return Err(stats
            .last_error
            .unwrap_or_else(|| "WOF compact failed.".into()));
    }
    let verb = match op {
        CompactOp::Compress => "Compacted",
        CompactOp::Uncompress => "Uncompacted",
    };
    Ok(CompactResult {
        ok: true,
        message: format!("{verb} with WOF /EXE."),
    })
}

/// If argv is `--wof-job <path>`, run the worker and return an exit code.
pub fn maybe_run_wof_job_cli() -> Option<i32> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--wof-job" {
            let path = args.next()?;
            return Some(run_wof_job_file(Path::new(&path)));
        }
    }
    None
}

pub fn run_wof_job_file(job_path: &Path) -> i32 {
    let raw = match std::fs::read_to_string(job_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Could not read WOF job: {e}");
            return 2;
        }
    };
    let job: WofJob = match serde_json::from_str(&raw) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("Could not parse WOF job: {e}");
            return 2;
        }
    };
    if let Err(err) = validate_job(&job) {
        let result = WofJobResult {
            last_error: Some(err.clone()),
            failed: 1,
            ..WofJobResult::default()
        };
        let _ = write_job_result(job_path, &result);
        eprintln!("{err}");
        return 2;
    }
    let result = run_job_files(&job);
    let _ = write_job_result(job_path, &result);
    if result.failed > 0 && result.ok == 0 && result.skipped == 0 {
        1
    } else {
        0
    }
}

fn write_job_result(job_path: &Path, result: &WofJobResult) -> std::io::Result<()> {
    std::fs::write(
        result_path_for_job(job_path),
        serde_json::to_vec_pretty(result)?,
    )
}

pub fn run_job_files(job: &WofJob) -> WofJobResult {
    let op = CompactOp::from(job.op);
    let cluster = effective_cluster(&job.root);
    let workers = worker_count(job.algorithm, volume_seek_penalty(&job.root));
    let stats = run_files_parallel(
        op,
        &job.root,
        &job.files,
        job.algorithm,
        job.force,
        cluster,
        workers,
        &mut |_| {},
    );
    let leftover = stats.denied.len();
    WofJobResult {
        ok: stats.ok,
        failed: stats.failed.saturating_add(leftover),
        skipped: stats.skipped,
        last_error: stats
            .last_error
            .or_else(|| (leftover > 0).then(|| "Access is denied.".into())),
    }
}

fn elevate_and_run(job: &WofJob) -> Result<WofJobResult, String> {
    #[cfg(test)]
    {
        super::wof::test_set_elevated(true);
        let result = run_job_files(job);
        super::wof::test_set_elevated(false);
        Ok(result)
    }
    #[cfg(all(windows, not(test)))]
    {
        return windows_elevate_and_run(job);
    }
    #[cfg(all(not(windows), not(test)))]
    {
        Ok(run_job_files(job))
    }
}

#[cfg(all(windows, not(test)))]
fn windows_elevate_and_run(job: &WofJob) -> Result<WofJobResult, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
    use windows::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    let exe = std::env::current_exe().map_err(|e| format!("Could not locate rusticgu.exe: {e}"))?;
    let job_path = std::env::temp_dir().join(format!(
        "rusticgu-wof-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(
        &job_path,
        serde_json::to_vec_pretty(job).map_err(|e| format!("Could not write WOF job: {e}"))?,
    )
    .map_err(|e| format!("Could not write WOF job: {e}"))?;

    let file = wide(exe.as_os_str());
    let verb = wide(std::ffi::OsStr::new("runas"));
    let params = format!("--wof-job \"{}\"", job_path.display());
    let params_w = wide(std::ffi::OsStr::new(&params));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params_w.as_ptr()),
        nShow: SW_HIDE.0 as i32,
        ..Default::default()
    };
    let ok = unsafe { ShellExecuteExW(&mut info) };
    if ok.is_err() {
        let _ = std::fs::remove_file(&job_path);
        return Err("Could not elevate WOF worker (UAC cancelled or failed).".into());
    }
    let mut exit = 1u32;
    if !info.hProcess.is_invalid() {
        unsafe {
            let _ = WaitForSingleObject(info.hProcess, INFINITE);
            let _ = GetExitCodeProcess(info.hProcess, &mut exit);
            let _ = CloseHandle(info.hProcess);
        }
    }
    let parsed = std::fs::read_to_string(result_path_for_job(&job_path))
        .ok()
        .and_then(|s| serde_json::from_str::<WofJobResult>(&s).ok());
    let _ = std::fs::remove_file(&job_path);
    let _ = std::fs::remove_file(result_path_for_job(&job_path));
    interpret_job_result(exit, parsed.as_ref(), job.op.into())?;
    Ok(parsed.unwrap_or_default())
}

fn resolve_explicit(root: &Path, files: &[PathBuf]) -> Vec<PathBuf> {
    files
        .iter()
        .map(|f| {
            if f.is_absolute() {
                f.clone()
            } else {
                root.join(f)
            }
        })
        .filter(|p| p.is_file() && !should_skip(p))
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn run_files_parallel(
    op: CompactOp,
    root: &Path,
    files: &[PathBuf],
    algorithm: CompactAlgorithm,
    force: bool,
    cluster: u64,
    workers: usize,
    progress: &mut (impl FnMut(CompactProgress) + Send),
) -> ApplyStats {
    let total = files.len().max(1);
    let processed = AtomicUsize::new(0);
    let ok = AtomicUsize::new(0);
    let skipped = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);
    let last_error = Mutex::new(None::<String>);
    let denied = Mutex::new(Vec::new());
    let progress = Mutex::new(progress);
    let stop = std::sync::atomic::AtomicBool::new(false);

    let workers = workers.max(1).min(files.len().max(1));
    std::thread::scope(|scope| {
        for chunk in split_files(files, workers) {
            let processed = &processed;
            let ok = &ok;
            let skipped = &skipped;
            let failed = &failed;
            let last_error = &last_error;
            let denied = &denied;
            let progress = &progress;
            let stop = &stop;
            scope.spawn(move || {
                for path in chunk {
                    let (outcome, err) =
                        process_file(op, root, path, algorithm, force, cluster, stop);
                    match outcome {
                        FileOutcome::Ok => {
                            ok.fetch_add(1, Ordering::Relaxed);
                        }
                        FileOutcome::Skipped => {
                            skipped.fetch_add(1, Ordering::Relaxed);
                        }
                        FileOutcome::AccessDenied => {
                            if let Ok(mut d) = denied.lock() {
                                d.push(path.clone());
                            }
                        }
                        FileOutcome::Failed => {
                            failed.fetch_add(1, Ordering::Relaxed);
                            if let Some(err) = err {
                                if let Ok(mut last) = last_error.lock() {
                                    *last = Some(err);
                                }
                            }
                        }
                    }
                    let n = processed.fetch_add(1, Ordering::Relaxed) + 1;
                    if let Ok(mut cb) = progress.lock() {
                        cb(CompactProgress {
                            processed: n.min(total),
                            total,
                            message: format!("WOF /EXE {n}/{total}…"),
                        });
                    }
                }
            });
        }
    });

    ApplyStats {
        ok: ok.load(Ordering::Relaxed),
        skipped: skipped.load(Ordering::Relaxed),
        failed: failed.load(Ordering::Relaxed),
        last_error: last_error.into_inner().ok().flatten(),
        denied: denied.into_inner().unwrap_or_default(),
    }
}

fn split_files(files: &[PathBuf], workers: usize) -> Vec<&[PathBuf]> {
    let workers = workers.max(1);
    let n = files.len();
    if n == 0 {
        return Vec::new();
    }
    let size = n.div_ceil(workers).max(1);
    files.chunks(size).collect()
}

fn process_file(
    op: CompactOp,
    root: &Path,
    path: &Path,
    algorithm: CompactAlgorithm,
    force: bool,
    cluster: u64,
    stop: &std::sync::atomic::AtomicBool,
) -> (FileOutcome, Option<String>) {
    if stop.load(Ordering::Relaxed) {
        return (FileOutcome::Skipped, None);
    }
    match op {
        CompactOp::Compress => process_compress(root, path, algorithm, force, cluster, stop),
        CompactOp::Uncompress => process_uncompress(root, path, stop),
    }
}

fn note_sharing(
    root: &Path,
    stop: &std::sync::atomic::AtomicBool,
) -> (FileOutcome, Option<String>) {
    if running_exe_in_tree(root).is_some() || crate::library::steam_updating_app_id(root).is_some()
    {
        stop.store(true, Ordering::Relaxed);
    }
    (FileOutcome::Skipped, None)
}

fn process_compress(
    root: &Path,
    path: &Path,
    algorithm: CompactAlgorithm,
    force: bool,
    cluster: u64,
    stop: &std::sync::atomic::AtomicBool,
) -> (FileOutcome, Option<String>) {
    let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if too_small(len, cluster) {
        return (FileOutcome::Skipped, None);
    }
    if !force {
        if let Ok(Some(existing)) = detect(path) {
            if same_wof_algorithm(existing, algorithm) {
                return (FileOutcome::Skipped, None);
            }
        }
    }
    if looks_incompressible(path, algorithm, CompactOp::Compress) && !force {
        return (FileOutcome::Skipped, None);
    }
    match compress_file(path, algorithm) {
        Ok(WofStatus::Applied | WofStatus::NotBeneficial | WofStatus::AlreadySame) => {
            (FileOutcome::Ok, None)
        }
        Err(WofError::AccessDenied) => (FileOutcome::AccessDenied, None),
        Err(WofError::SharingViolation) => note_sharing(root, stop),
        Err(_err) if algorithm == CompactAlgorithm::Lzx => {
            match compress_file(path, CompactAlgorithm::Xpress16k) {
                Ok(_) => (FileOutcome::Ok, None),
                Err(WofError::AccessDenied) => (FileOutcome::AccessDenied, None),
                Err(WofError::SharingViolation) => note_sharing(root, stop),
                Err(fallback) => (FileOutcome::Failed, Some(fallback.to_string())),
            }
        }
        Err(err) => (FileOutcome::Failed, Some(err.to_string())),
    }
}

fn process_uncompress(
    root: &Path,
    path: &Path,
    stop: &std::sync::atomic::AtomicBool,
) -> (FileOutcome, Option<String>) {
    match uncompress_file(path) {
        Ok(_) => (FileOutcome::Ok, None),
        Err(WofError::AccessDenied) => (FileOutcome::AccessDenied, None),
        Err(WofError::SharingViolation) => note_sharing(root, stop),
        Err(err) => (FileOutcome::Failed, Some(err.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compact::wof;

    fn stamp() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    }

    #[test]
    fn same_algorithm_is_skipped_unless_force() {
        let _wof_stub = wof::test_reset();
        let root = std::env::temp_dir().join(format!(
            "rusticgu-same-algo-{}-{}",
            std::process::id(),
            stamp()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let play = root.join("play.exe");
        std::fs::write(&play, vec![0u8; 64]).unwrap();
        wof::test_set_backing(&play, Some(CompactAlgorithm::Xpress8k));
        wof::test_set_ops(0);

        apply_wof(
            CompactOp::Compress,
            &root,
            CompactAlgorithm::Xpress8k,
            false,
            None,
            false,
            |_| {},
        )
        .unwrap();
        assert_eq!(wof::test_op_count(), 0, "same-algo skip must not compress");

        wof::test_set_ops(0);
        apply_wof(
            CompactOp::Compress,
            &root,
            CompactAlgorithm::Xpress8k,
            false,
            None,
            true,
            |_| {},
        )
        .unwrap();
        assert!(
            wof::test_op_count() >= 1,
            "force must rewrite already-compressed files"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tiny_files_are_skipped_when_cluster_is_4k() {
        let _wof_stub = wof::test_reset();
        wof::test_set_cluster(Some(4096));
        let root =
            std::env::temp_dir().join(format!("rusticgu-tiny-{}-{}", std::process::id(), stamp()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("play.exe"), vec![0u8; 100]).unwrap();
        apply_wof(
            CompactOp::Compress,
            &root,
            CompactAlgorithm::Xpress8k,
            false,
            None,
            false,
            |_| {},
        )
        .unwrap();
        assert_eq!(wof::test_op_count(), 0);
        let _ = std::fs::remove_dir_all(&root);
        wof::test_set_cluster(None);
    }

    #[test]
    fn not_beneficial_counts_as_ok() {
        let _wof_stub = wof::test_reset();
        let root =
            std::env::temp_dir().join(format!("rusticgu-nb-{}-{}", std::process::id(), stamp()));
        std::fs::create_dir_all(&root).unwrap();
        let play = root.join("play.exe");
        std::fs::write(&play, vec![0u8; 64]).unwrap();
        wof::test_set_not_beneficial(&play, true);
        let result = apply_wof(
            CompactOp::Compress,
            &root,
            CompactAlgorithm::Xpress8k,
            false,
            None,
            false,
            |_| {},
        )
        .unwrap();
        assert!(result.ok);
        assert!(wof::test_op_count() >= 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn one_hard_failure_does_not_abort_the_title() {
        let _wof_stub = wof::test_reset();
        let root =
            std::env::temp_dir().join(format!("rusticgu-surv-{}-{}", std::process::id(), stamp()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("play.exe"), vec![0u8; 64]).unwrap();
        let bad = root.join("data.dat");
        std::fs::write(&bad, vec![0u8; 64]).unwrap();
        wof::test_set_hard_fail(&bad, true);
        let result = apply_wof(
            CompactOp::Compress,
            &root,
            CompactAlgorithm::Xpress8k,
            false,
            None,
            false,
            |_| {},
        )
        .unwrap();
        assert!(result.ok);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn uncompact_targets_only_wof_backed_files() {
        let _wof_stub = wof::test_reset();
        let root =
            std::env::temp_dir().join(format!("rusticgu-un-{}-{}", std::process::id(), stamp()));
        std::fs::create_dir_all(&root).unwrap();
        let play = root.join("play.exe");
        let movie = root.join("movie.mp4");
        let other = root.join("level.dat");
        std::fs::write(&play, vec![0u8; 64]).unwrap();
        std::fs::write(&movie, vec![0u8; 64]).unwrap();
        std::fs::write(&other, vec![0u8; 64]).unwrap();
        wof::test_set_backing(&play, Some(CompactAlgorithm::Xpress8k));
        wof::test_set_backing(&movie, Some(CompactAlgorithm::Xpress8k));

        let targets = collect_wof_backed_files(&root);
        let names: Vec<String> = targets
            .iter()
            .filter_map(|p| p.file_name()?.to_str().map(|s| s.to_string()))
            .collect();
        assert!(names.iter().any(|n| n == "play.exe"));
        assert!(
            names.iter().any(|n| n == "movie.mp4"),
            "previously compacted media must still uncompact: {names:?}"
        );
        assert!(!names.iter().any(|n| n == "level.dat"));

        wof::test_set_ops(0);
        apply_wof(
            CompactOp::Uncompress,
            &root,
            CompactAlgorithm::Xpress8k,
            false,
            None,
            false,
            |_| {},
        )
        .unwrap();
        assert_eq!(wof::test_op_count(), 2);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn incompressible_probe_skips_without_wof() {
        let _wof_stub = wof::test_reset();
        let root =
            std::env::temp_dir().join(format!("rusticgu-probe-{}-{}", std::process::id(), stamp()));
        std::fs::create_dir_all(&root).unwrap();
        let blob = root.join("huge.dat");
        std::fs::write(&blob, vec![0u8; 64]).unwrap();
        wof::test_set_incompressible(&blob, true);
        apply_wof(
            CompactOp::Compress,
            &root,
            CompactAlgorithm::Lzx,
            false,
            None,
            false,
            |_| {},
        )
        .unwrap();
        assert_eq!(wof::test_op_count(), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn access_denied_retries_elevated() {
        let _wof_stub = wof::test_reset();
        let root =
            std::env::temp_dir().join(format!("rusticgu-acl-{}-{}", std::process::id(), stamp()));
        std::fs::create_dir_all(&root).unwrap();
        let play = root.join("play.exe");
        std::fs::write(&play, vec![0u8; 64]).unwrap();
        wof::test_set_access_denied(&play, true);
        let result = apply_wof(
            CompactOp::Compress,
            &root,
            CompactAlgorithm::Xpress8k,
            false,
            None,
            false,
            |_| {},
        )
        .unwrap();
        assert!(result.ok);
        assert!(
            wof::test_op_count() >= 2,
            "in-process deny then elevated retry, got {}",
            wof::test_op_count()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sharing_violation_skips_and_does_not_abort() {
        let _wof_stub = wof::test_reset();
        let root =
            std::env::temp_dir().join(format!("rusticgu-share-{}-{}", std::process::id(), stamp()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("play.exe"), vec![0u8; 64]).unwrap();
        let locked = root.join("data.dat");
        std::fs::write(&locked, vec![0u8; 64]).unwrap();
        wof::test_set_sharing(&locked, true);
        let result = apply_wof(
            CompactOp::Compress,
            &root,
            CompactAlgorithm::Xpress8k,
            false,
            None,
            false,
            |_| {},
        )
        .unwrap();
        assert!(result.ok);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn native_uncompact_does_not_use_recursive_s() {
        use crate::compact::command::{build_apply_invocations, invocation_recurses_install_root};

        let _wof_stub = wof::test_reset();
        let root = std::env::temp_dir().join(format!(
            "rusticgu-native-un-{}-{}",
            std::process::id(),
            stamp()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let play = root.join("play.exe");
        std::fs::write(&play, vec![0u8; 64]).unwrap();
        wof::test_set_backing(&play, Some(CompactAlgorithm::Xpress8k));

        let fallback =
            build_apply_invocations(CompactOp::Uncompress, &root, CompactAlgorithm::Xpress8k);
        assert!(
            fallback
                .iter()
                .any(|inv| invocation_recurses_install_root(inv, &root)),
            "compact.exe fallback may still bind /S:<root>"
        );

        wof::test_set_ops(0);
        apply_wof(
            CompactOp::Uncompress,
            &root,
            CompactAlgorithm::Xpress8k,
            false,
            None,
            false,
            |_| {},
        )
        .unwrap();
        assert_eq!(wof::test_op_count(), 1);
        assert_eq!(collect_wof_backed_files(&root).len(), 0);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn wof_job_file_writes_result_and_exit_zero() {
        let _wof_stub = wof::test_reset();
        let root = std::env::temp_dir().join(format!(
            "rusticgu-jobfile-{}-{}",
            std::process::id(),
            stamp()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let play = root.join("play.exe");
        std::fs::write(&play, vec![0u8; 64]).unwrap();
        let job_path = root.join("job.json");
        let job = WofJob {
            op: JobOp::Compress,
            algorithm: CompactAlgorithm::Xpress8k,
            force: false,
            root: root.clone(),
            files: vec![play.clone()],
        };
        std::fs::write(&job_path, serde_json::to_vec_pretty(&job).unwrap()).unwrap();
        let code = run_wof_job_file(&job_path);
        assert_eq!(code, 0);
        let parsed: WofJobResult =
            serde_json::from_str(&std::fs::read_to_string(result_path_for_job(&job_path)).unwrap())
                .unwrap();
        assert!(parsed.ok >= 1);
        assert_eq!(parsed.failed, 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn leftover_elevated_denials_are_failures() {
        let _wof_stub = wof::test_reset();
        let root = std::env::temp_dir().join(format!(
            "rusticgu-sticky-acl-{}-{}",
            std::process::id(),
            stamp()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let play = root.join("play.exe");
        std::fs::write(&play, vec![0u8; 64]).unwrap();
        wof::test_set_always_denied(&play, true);
        let err = apply_wof(
            CompactOp::Compress,
            &root,
            CompactAlgorithm::Xpress8k,
            false,
            None,
            false,
            |_| {},
        )
        .unwrap_err();
        assert!(
            err.to_ascii_lowercase().contains("denied"),
            "still-denied files must not report compact success: {err}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn force_rewrites_incompressible_files() {
        let _wof_stub = wof::test_reset();
        let root = std::env::temp_dir().join(format!(
            "rusticgu-force-probe-{}-{}",
            std::process::id(),
            stamp()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let blob = root.join("huge.dat");
        std::fs::write(&blob, vec![0u8; 64]).unwrap();
        wof::test_set_incompressible(&blob, true);
        wof::test_set_backing(&blob, Some(CompactAlgorithm::Lzx));
        wof::test_set_ops(0);
        apply_wof(
            CompactOp::Compress,
            &root,
            CompactAlgorithm::Xpress8k,
            false,
            None,
            true,
            |_| {},
        )
        .unwrap();
        assert!(
            wof::test_op_count() >= 1,
            "Change-method must still rewrite incompressible files"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
