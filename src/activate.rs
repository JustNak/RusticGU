//! Minimal named-pipe activate server (show-window only).
//!
//! Second launches ask the primary instance to restore its window. This is not
//! a general IPC protocol. Native-host / extension traffic is out of scope.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Shared flag set when another process asks the primary window to show.
#[derive(Clone, Debug)]
pub struct ActivateBridge {
    show_window: Arc<AtomicBool>,
}

impl ActivateBridge {
    pub fn new() -> Self {
        Self {
            show_window: Arc::new(AtomicBool::new(false)),
        }
    }

    #[allow(dead_code)]
    pub fn request_show(&self) {
        self.show_window.store(true, Ordering::SeqCst);
    }

    pub fn take_show_window_request(&self) -> bool {
        self.show_window.swap(false, Ordering::SeqCst)
    }
}

/// Start a best-effort named-pipe listener that answers `show_window`.
pub fn start_activate_server(bridge: ActivateBridge) {
    #[cfg(windows)]
    {
        std::thread::Builder::new()
            .name("rusticgu-activate".into())
            .spawn(move || windows_impl::serve(bridge))
            .ok();
    }
    #[cfg(not(windows))]
    {
        let _ = bridge;
    }
}

#[cfg(windows)]
mod windows_impl {
    use super::ActivateBridge;
    use crate::branding::PIPE_NAME;
    use std::io::{BufRead, BufReader, Write};
    use std::os::windows::io::FromRawHandle;

    pub(super) fn serve(bridge: ActivateBridge) {
        loop {
            match accept_once() {
                Ok(mut stream) => {
                    let mut reader = BufReader::new(&mut stream);
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_ok() && looks_like_show_window(&line) {
                        bridge.request_show();
                        let _ = writeln!(stream, r#"{{"ok":true}}"#);
                    } else {
                        let _ = writeln!(stream, r#"{{"ok":false}}"#);
                    }
                }
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
            }
        }
    }

    fn looks_like_show_window(line: &str) -> bool {
        let trimmed = line.trim();
        trimmed.contains("\"show_window\"") || trimmed.contains("\"type\":\"show_window\"")
    }

    fn accept_once() -> std::io::Result<std::fs::File> {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows::Win32::Storage::FileSystem::{FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX};
        use windows::Win32::System::Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE,
            PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
        };

        let wide: Vec<u16> = std::ffi::OsStr::new(PIPE_NAME)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let handle = unsafe {
            CreateNamedPipeW(
                PCWSTR(wide.as_ptr()),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                4096,
                4096,
                0,
                None,
            )
        };
        if handle.is_invalid() || handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }
        let connected = unsafe { ConnectNamedPipe(handle, None) };
        if connected.is_err() {
            // ERROR_PIPE_CONNECTED (535) is success when a client is already waiting.
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() != Some(535) {
                return Err(err);
            }
        }
        Ok(unsafe { std::fs::File::from_raw_handle(handle.0 as *mut std::ffi::c_void) })
    }
}
