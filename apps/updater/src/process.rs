//! Process wait + single-instance helpers.

use std::time::Duration;
#[cfg(windows)]
use std::time::Instant;

use crate::ui::ProgressSink;

const MUTEX_NAME: &str = "Local\\RusticGU.Updater";

/// Returns true if this process owns the single-instance mutex.
pub fn try_acquire_single_instance() -> bool {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
        use windows::Win32::System::Threading::CreateMutexW;

        let wide: Vec<u16> = MUTEX_NAME
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let result = unsafe { CreateMutexW(None, true, PCWSTR(wide.as_ptr())) };
        match result {
            Ok(_handle) => {
                let err = unsafe { GetLastError() };
                err != ERROR_ALREADY_EXISTS
            }
            Err(_) => true,
        }
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// Wait until `pid` exits, or until `timeout`.
pub fn wait_for_process_exit(
    pid: u32,
    timeout: Duration,
    progress: &dyn ProgressSink,
) -> Result<(), WaitError> {
    progress.set_status("Waiting for RusticGU to close…".into());
    progress.set_progress_unknown();

    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0, WAIT_TIMEOUT};
        use windows::Win32::System::Threading::{
            OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
        };

        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) };
        let handle = match handle {
            Ok(h) if !h.is_invalid() => h,
            _ => {
                return Ok(());
            }
        };

        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                unsafe {
                    let _ = CloseHandle(handle);
                }
                return Err(WaitError::Timeout);
            }
            let ms = remaining.as_millis().min(u32::MAX as u128) as u32;
            let slice = ms.min(250);
            let wait = unsafe { WaitForSingleObject(handle, slice) };
            if wait == WAIT_OBJECT_0 {
                unsafe {
                    let _ = CloseHandle(handle);
                }
                std::thread::sleep(Duration::from_millis(400));
                return Ok(());
            }
            if wait == WAIT_TIMEOUT {
                continue;
            }
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Ok(());
        }
    }

    #[cfg(not(windows))]
    {
        let _ = (pid, timeout, progress);
        Err(WaitError::Timeout)
    }
}

#[derive(Debug)]
pub enum WaitError {
    Timeout,
}
