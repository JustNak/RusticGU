//! Tray lifecycle, close-to-tray, and SW_HIDE restore.

use gpui::{Context, Window};

use super::LibraryApp;
use crate::settings::OsNotifyMode;
use crate::tray::{
    hide_main_window, main_window_hwnd, show_main_window, show_main_window_hwnd, SystemTray,
    TrayEvent,
};

impl LibraryApp {
    pub(crate) fn handle_window_should_close(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.force_quit || !self.settings.close_to_tray {
            self.flush_window_layout_now();
            self.flush_state_now();
            return true;
        }

        self.ensure_tray(cx);
        if self.system_tray.is_none() {
            self.flush_window_layout_now();
            self.flush_state_now();
            return true;
        }

        self.flush_window_layout_now();
        let hwnd = main_window_hwnd(window);
        if hwnd != 0 {
            self.main_hwnd = hwnd;
        }
        hide_main_window(window);
        self.window_hidden_to_tray = true;
        self.close_flyout(cx);
        cx.notify();
        false
    }

    fn ensure_tray(&mut self, cx: &mut Context<Self>) {
        if self.system_tray.is_some() {
            return;
        }
        let (tray_tx, tray_rx) = async_channel::unbounded::<TrayEvent>();
        self.system_tray = SystemTray::start(tray_tx);
        if self.system_tray.is_none() {
            return;
        }
        cx.spawn(async move |this, cx| {
            while let Ok(event) = tray_rx.recv().await {
                let result = this.update(cx, |app, cx| app.handle_tray_event(event, cx));
                if result.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn stop_tray(&mut self) {
        self.system_tray = None;
    }

    fn stop_tray_nonblocking(&mut self) {
        if let Some(tray) = self.system_tray.take() {
            let _ = std::thread::Builder::new()
                .name("rusticgu-tray-shutdown".into())
                .spawn(move || drop(tray));
        }
    }

    /// Full quit. Used by the flyout footer Exit button (QA FAIL #3) and by
    /// the tray-menu exit command, not only the tray menu.
    pub(crate) fn force_quit_app(&mut self, cx: &mut Context<Self>) {
        self.force_quit = true;
        self.flush_window_layout_now();
        self.flush_state_now();
        self.close_flyout(cx);
        self.stop_tray_nonblocking();
        cx.quit();
    }

    pub(crate) fn sync_tray_lifetime(&mut self, cx: &mut Context<Self>) {
        let needed = self.settings.close_to_tray
            || self.window_hidden_to_tray
            || self.settings.os_notify_mode != OsNotifyMode::Off;
        if needed {
            self.ensure_tray(cx);
        } else {
            self.stop_tray();
        }
    }

    pub(crate) fn handle_tray_event(&mut self, event: TrayEvent, cx: &mut Context<Self>) {
        match event {
            TrayEvent::ShowWindow => {
                self.restore_main_window_now();
                self.pending_tray_show = true;
                self.close_flyout(cx);
                cx.notify();
            }
            TrayEvent::ToggleFlyout => {
                // Open from the event, never from Render. A nested open_window
                // during LibraryApp::render leaves an empty frameless HWND.
                self.toggle_flyout(cx);
            }
            TrayEvent::Exit => {
                self.force_quit_app(cx);
            }
            TrayEvent::BalloonUserClick { .. } => {
                self.restore_main_window_now();
                self.pending_tray_show = true;
                cx.notify();
            }
        }
    }

    fn restore_main_window_now(&mut self) {
        self.window_hidden_to_tray = false;
        if self.main_hwnd != 0 {
            show_main_window_hwnd(self.main_hwnd);
        }
    }

    pub(crate) fn poll_hidden_window_actions(&mut self, cx: &mut Context<Self>) {
        if self.activate.take_show_window_request() {
            self.restore_main_window_now();
            self.pending_tray_show = true;
            cx.notify();
        }
    }

    pub(crate) fn apply_pending_tray_actions(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let hwnd = main_window_hwnd(window);
        if hwnd != 0 {
            self.main_hwnd = hwnd;
        }
        if self.pending_tray_show {
            self.pending_tray_show = false;
            self.window_hidden_to_tray = false;
            show_main_window(window);
        }
        if self.pending_toggle_flyout {
            self.pending_toggle_flyout = false;
        }
        if self.pending_open_compact {
            self.pending_open_compact = false;
            self.open_compact_flow(window, cx);
        }
    }
}
