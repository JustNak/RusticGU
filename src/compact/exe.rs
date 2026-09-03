//! Last-resort `compact.exe` fallback when `WofUtil.dll` is missing.

use std::path::{Path, PathBuf};

use crate::settings::CompactAlgorithm;

use super::command::{
    build_apply_invocations_with_force, build_incremental_invocations,
    build_wof_files_command_with, invocation_target_files, CompactInvocation, CompactOp,
};
use super::engine::{CompactProgress, CompactResult};

struct CommandOutput {
    status_ok: bool,
    stdout: String,
    stderr: String,
    code: Option<i32>,
}

pub fn apply_via_compact_exe(
    op: CompactOp,
    root: &Path,
    algorithm: CompactAlgorithm,
    explicit_files: Option<&[PathBuf]>,
    force: bool,
    mut progress: impl FnMut(CompactProgress) + Send,
) -> Result<CompactResult, String> {
    let invocations = match explicit_files {
        Some(files) => build_incremental_invocations(root, files, algorithm),
        None => {
            let coerce_live = algorithm.is_live();
            build_apply_invocations_with_force(op, root, algorithm, coerce_live, force)
        }
    };
    if invocations.is_empty() {
        return Ok(CompactResult {
            ok: true,
            message: "Nothing to compact (skip list excluded every file).".into(),
        });
    }

    let total = invocations
        .iter()
        .map(|inv| invocation_target_files(inv).len().max(1))
        .sum::<usize>()
        .max(1);
    let mut elevate = false;
    let mut processed = 0usize;
    let mut last_output = CommandOutput {
        status_ok: true,
        stdout: String::new(),
        stderr: String::new(),
        code: Some(0),
    };
    let mut last_err: Option<String> = None;
    let mut any_ok = false;

    for inv in &invocations {
        let file_n = invocation_target_files(inv).len().max(1);
        progress(CompactProgress {
            processed: processed.min(total),
            total,
            message: format!("WOF /EXE {processed}/{total}…"),
        });
        match run_compact_access(
            op,
            algorithm,
            force,
            &mut elevate,
            processed,
            total,
            inv,
            &mut progress,
        ) {
            Ok(output) if output.status_ok => {
                any_ok = true;
                last_output = output;
            }
            Ok(output) => match recover_failed(
                op,
                algorithm,
                force,
                &mut elevate,
                processed,
                total,
                inv,
                &mut progress,
                output,
            ) {
                Ok(output) => {
                    any_ok = true;
                    last_output = output;
                }
                Err(err) => last_err = Some(err),
            },
            Err(err) => last_err = Some(err),
        }
        processed = processed.saturating_add(file_n);
    }

    progress(CompactProgress {
        processed: total,
        total,
        message: "Finished.".into(),
    });
    if let Some(err) = last_err {
        if !any_ok {
            return Err(err);
        }
    }
    if last_output.status_ok {
        let verb = match op {
            CompactOp::Compress => "Compacted",
            CompactOp::Uncompress => "Uncompacted",
        };
        Ok(CompactResult {
            ok: true,
            message: format!("{verb} with WOF /EXE."),
        })
    } else {
        Err(output_error(&last_output))
    }
}

fn is_access_denied(output: &CommandOutput) -> bool {
    let blob = format!("{}\n{}", output.stdout, output.stderr).to_ascii_lowercase();
    output.code == Some(5) || blob.contains("access is denied") || blob.contains("access denied")
}

fn output_error(output: &CommandOutput) -> String {
    let detail = if output.stderr.trim().is_empty() {
        output.stdout.trim().to_string()
    } else {
        output.stderr.trim().to_string()
    };
    if detail.is_empty() {
        format!("compact.exe failed (exit {}).", output.code.unwrap_or(-1))
    } else {
        detail
    }
}

fn run_compact_access(
    op: CompactOp,
    algorithm: CompactAlgorithm,
    force: bool,
    elevate: &mut bool,
    processed: usize,
    total: usize,
    inv: &CompactInvocation,
    progress: &mut (impl FnMut(CompactProgress) + Send),
) -> Result<CommandOutput, String> {
    let _ = (op, algorithm, force);
    let mut output = run_compact(inv, *elevate)?;
    if is_access_denied(&output) && !*elevate {
        progress(CompactProgress {
            processed,
            total,
            message: "Access denied. Retrying elevated…".into(),
        });
        *elevate = true;
        output = run_compact(inv, true)?;
    }
    Ok(output)
}

fn recover_failed(
    op: CompactOp,
    algorithm: CompactAlgorithm,
    force: bool,
    elevate: &mut bool,
    processed: usize,
    total: usize,
    inv: &CompactInvocation,
    progress: &mut (impl FnMut(CompactProgress) + Send),
    failed: CommandOutput,
) -> Result<CommandOutput, String> {
    let files = invocation_target_files(inv);
    if files.len() > 1 {
        let mut last_ok = None;
        let mut last_err = None;
        for file in files {
            let single =
                build_wof_files_command_with(op, std::slice::from_ref(&file), algorithm, force);
            match run_compact_access(
                op, algorithm, force, elevate, processed, total, &single, progress,
            ) {
                Ok(out) if out.status_ok => last_ok = Some(out),
                Ok(out) => last_err = Some(output_error(&out)),
                Err(err) => last_err = Some(err),
            }
        }
        if let Some(out) = last_ok {
            return Ok(out);
        }
        return Err(last_err.unwrap_or_else(|| output_error(&failed)));
    }
    if op == CompactOp::Compress && algorithm == CompactAlgorithm::Lzx && !files.is_empty() {
        let fallback = build_wof_files_command_with(op, &files, CompactAlgorithm::Xpress16k, force);
        match run_compact_access(
            op,
            CompactAlgorithm::Xpress16k,
            force,
            elevate,
            processed,
            total,
            &fallback,
            progress,
        ) {
            Ok(out) if out.status_ok => return Ok(out),
            Ok(out) => return Err(output_error(&out)),
            Err(err) => return Err(err),
        }
    }
    Err(output_error(&failed))
}

fn run_compact(inv: &CompactInvocation, elevate: bool) -> Result<CommandOutput, String> {
    #[cfg(windows)]
    {
        windows_run(inv, elevate)
    }
    #[cfg(not(windows))]
    {
        let _ = elevate;
        Ok(CommandOutput {
            status_ok: true,
            stdout: format!("dry {}", inv.display_cmdline()),
            stderr: String::new(),
            code: Some(0),
        })
    }
}

#[cfg(windows)]
fn windows_run(inv: &CompactInvocation, elevate: bool) -> Result<CommandOutput, String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    if elevate {
        return windows_run_elevated(inv);
    }

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = Command::new(&inv.program)
        .args(&inv.args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Could not start compact.exe: {e}"))?;
    Ok(CommandOutput {
        status_ok: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code(),
    })
}

#[cfg(windows)]
fn windows_run_elevated(inv: &CompactInvocation) -> Result<CommandOutput, String> {
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

    let file = wide(std::ffi::OsStr::new("compact.exe"));
    let verb = wide(std::ffi::OsStr::new("runas"));
    let mut params = String::new();
    for (i, arg) in inv.args.iter().enumerate() {
        if i > 0 {
            params.push(' ');
        }
        let s = arg.to_string_lossy();
        if s.chars().any(|c| c.is_whitespace()) {
            params.push('"');
            params.push_str(&s);
            params.push('"');
        } else {
            params.push_str(&s);
        }
    }
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
        return Err("Could not elevate compact.exe (UAC cancelled or failed).".into());
    }
    let mut code = 1u32;
    if !info.hProcess.is_invalid() {
        unsafe {
            let _ = WaitForSingleObject(info.hProcess, INFINITE);
            let _ = GetExitCodeProcess(info.hProcess, &mut code);
            let _ = CloseHandle(info.hProcess);
        }
    }
    Ok(CommandOutput {
        status_ok: code == 0,
        stdout: String::new(),
        stderr: String::new(),
        code: Some(code as i32),
    })
}
