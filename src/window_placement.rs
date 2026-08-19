//! Resolution-aware native window placement helpers.
//!
//! GPUI's `Bounds::centered` + Windows `SetWindowPlacement` path can leave
//! newly created windows (especially `WindowKind::PopUp`) at the cascade /
//! corner position chosen by `CreateWindowEx(CW_USEDEFAULT)`. These helpers
//! re-center using the monitor work area in physical screen coordinates, which
//! is correct across DPI scales and multi-monitor layouts.

use gpui::{point, px, App, Point, Window};

use crate::tray::TrayIconAnchor;

/// Logical size of the tray flyout panel.
pub const FLYOUT_WIDTH_PX: i32 = 308;
pub const FLYOUT_HEIGHT_PX: i32 = 400;
/// Extra gap above the Windows work-area bottom so the panel clears the taskbar.
pub const FLYOUT_TASKBAR_CLEARANCE_PX: i32 = 20;

/// Pixel step used to stagger stacked capture HUDs so the Nth window is visible.
pub const CASCADE_STEP_PX: i32 = 36;

/// Center a GPUI window on the most appropriate monitor work area.
///
/// Prefer the monitor under the mouse cursor (where the user is looking /
/// interacting). Fall back to the monitor that currently hosts the window,
/// then to the primary display.
pub fn center_window(window: &Window) {
    #[cfg(windows)]
    {
        if let Some(hwnd) = hwnd_of(window) {
            center_hwnd(hwnd);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = window;
    }
}

/// Center, then offset by `index * CASCADE_STEP_PX` so multiple HUDs do not stack.
pub fn cascade_window(window: &Window, index: usize) {
    #[cfg(windows)]
    {
        if let Some(hwnd) = hwnd_of(window) {
            cascade_hwnd(hwnd, index);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (window, index);
    }
}

/// Center a raw Win32 window handle on the cursor/host/primary work area.
#[cfg(windows)]
pub fn center_hwnd(hwnd: windows::Win32::Foundation::HWND) {
    cascade_hwnd(hwnd, 0);
}

/// Center, then stagger by `index` cascade steps, clamped to the work area.
#[cfg(windows)]
pub fn cascade_hwnd(hwnd: windows::Win32::Foundation::HWND, index: usize) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, SetWindowPos, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
    };

    unsafe {
        if hwnd.0.is_null() {
            return;
        }

        let mut window_rect = RECT::default();
        if GetWindowRect(hwnd, &mut window_rect).is_err() {
            return;
        }

        let width = window_rect.right - window_rect.left;
        let height = window_rect.bottom - window_rect.top;
        if width <= 0 || height <= 0 {
            return;
        }

        let monitor = monitor_for_centering(hwnd, &window_rect);
        if monitor.0.is_null() {
            return;
        }

        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }

        // `rcWork` excludes the taskbar / docked bars — better than full `rcMonitor`.
        let work = info.rcWork;
        let (x, y) = cascade_origin(
            work.left,
            work.top,
            work.right - work.left,
            work.bottom - work.top,
            width,
            height,
            index,
        );

        let _ = SetWindowPos(
            hwnd,
            None,
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// Centered origin plus cascade offset, clamped so the window stays in the work area.
pub fn cascade_origin(
    work_left: i32,
    work_top: i32,
    work_w: i32,
    work_h: i32,
    width: i32,
    height: i32,
    index: usize,
) -> (i32, i32) {
    if work_w <= 0 || work_h <= 0 || width <= 0 || height <= 0 {
        return (work_left, work_top);
    }
    let (dx, dy) = cascade_delta(index);
    let centered_x = work_left + (work_w - width) / 2;
    let centered_y = work_top + (work_h - height) / 2;
    let max_x = work_left + (work_w - width).max(0);
    let max_y = work_top + (work_h - height).max(0);
    (
        (centered_x + dx).clamp(work_left, max_x),
        (centered_y + dy).clamp(work_top, max_y),
    )
}

/// Offset from the centered origin for the Nth stacked HUD.
pub fn cascade_delta(index: usize) -> (i32, i32) {
    let step = CASCADE_STEP_PX.saturating_mul(index as i32);
    (step, step)
}

#[cfg(windows)]
fn hwnd_of(window: &Window) -> Option<windows::Win32::Foundation::HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;

    let handle = <Window as HasWindowHandle>::window_handle(window).ok()?;
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return None;
    };
    Some(HWND(win32.hwnd.get() as *mut core::ffi::c_void))
}

/// Pick the monitor to center on: cursor → window host → primary.
#[cfg(windows)]
unsafe fn monitor_for_centering(
    hwnd: windows::Win32::Foundation::HWND,
    window_rect: &windows::Win32::Foundation::RECT,
) -> windows::Win32::Graphics::Gdi::HMONITOR {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{
        MonitorFromPoint, MonitorFromRect, MonitorFromWindow, MONITOR_DEFAULTTONEAREST,
        MONITOR_DEFAULTTOPRIMARY,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let mut cursor = POINT::default();
    if GetCursorPos(&mut cursor).is_ok() {
        let from_cursor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        if !from_cursor.0.is_null() {
            return from_cursor;
        }
    }

    let from_window = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    if !from_window.0.is_null() {
        return from_window;
    }

    let from_rect = MonitorFromRect(window_rect, MONITOR_DEFAULTTONEAREST);
    if !from_rect.0.is_null() {
        return from_rect;
    }

    MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY)
}

/// Place a flyout HWND above the tray icon, fully inside the monitor work area.
pub fn place_flyout_above_tray(
    window: &Window,
    anchor: Option<TrayIconAnchor>,
    width: i32,
    height: i32,
    extra_clearance: i32,
) {
    #[cfg(windows)]
    {
        if let Some(hwnd) = hwnd_of(window) {
            place_flyout_hwnd(hwnd, anchor, width, height, extra_clearance);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (window, anchor, width, height, extra_clearance);
    }
}

/// Fallback origin used as the initial `window_bounds` so GPUI's first draw
/// has a real size (not a transparent 0-content popup).
pub fn flyout_fallback_origin(cx: &App) -> Point<gpui::Pixels> {
    if let Some(display) = cx.displays().first() {
        let bounds = display.bounds();
        return point(
            bounds.origin.x + bounds.size.width
                - px((FLYOUT_WIDTH_PX + FLYOUT_TASKBAR_CLEARANCE_PX) as f32),
            bounds.origin.y + bounds.size.height
                - px((FLYOUT_HEIGHT_PX + FLYOUT_TASKBAR_CLEARANCE_PX + 48) as f32),
        );
    }
    point(px(24.), px(24.))
}

/// Compute a flyout origin that sits above `anchor` and stays inside `work`.
///
/// `work` is `(left, top, right, bottom)` in screen pixels (`rcWork` — already
/// excludes the taskbar). `anchor` is `(x, y, w, h)` of the tray icon or cursor.
/// `extra_clearance` is added above the work-area bottom so the panel cannot
/// clip into the taskbar.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn flyout_origin_in_work_area(
    work: (i32, i32, i32, i32),
    anchor: (i32, i32, i32, i32),
    flyout_w: i32,
    flyout_h: i32,
    extra_clearance: i32,
) -> (i32, i32) {
    let (work_left, work_top, work_right, work_bottom) = work;
    let (anchor_x, anchor_y, anchor_w, anchor_h) = anchor;
    let work_w = (work_right - work_left).max(0);
    let work_h = (work_bottom - work_top).max(0);
    let flyout_w = flyout_w.max(1);
    let flyout_h = flyout_h.max(1);
    let extra_clearance = extra_clearance.max(0);

    let max_x = work_left + (work_w - flyout_w).max(0);
    let max_y = work_top + (work_h - extra_clearance - flyout_h).max(0);

    // Right-align to the icon; tray icons sit at the right of the taskbar.
    let x = (anchor_x + anchor_w - flyout_w).clamp(work_left, max_x);

    // Prefer immediately above the icon. Tray icons usually live *below* rcWork.
    let mut y = anchor_y - extra_clearance - flyout_h;
    if anchor_y + anchor_h > work_bottom || y + flyout_h > work_bottom - extra_clearance {
        y = work_bottom - extra_clearance - flyout_h;
    }
    if y < work_top {
        y = (anchor_y + anchor_h + extra_clearance).min(max_y);
    }
    y = y.clamp(work_top, max_y);
    (x, y)
}

#[cfg(windows)]
fn place_flyout_hwnd(
    hwnd: windows::Win32::Foundation::HWND,
    anchor: Option<TrayIconAnchor>,
    width: i32,
    height: i32,
    extra_clearance: i32,
) {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetWindowLongW, SetForegroundWindow, SetWindowLongW, SetWindowPos,
        ShowWindow, GWL_EXSTYLE, GWL_STYLE, HWND_TOPMOST, SWP_FRAMECHANGED, SWP_SHOWWINDOW,
        SW_SHOWNA, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_EX_APPWINDOW, WS_EX_LAYERED,
        WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
    };

    unsafe {
        if hwnd.0.is_null() {
            return;
        }

        // gpui 0.2.2 PopUp windows are created with WINDOW_STYLE(0). Give the
        // HWND a real popup style so children can paint (not an empty frame).
        let style = (WS_POPUP | WS_VISIBLE | WS_CLIPCHILDREN | WS_CLIPSIBLINGS).0 as i32;
        SetWindowLongW(hwnd, GWL_STYLE, style);
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        let ex =
            (ex | WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0) & !WS_EX_APPWINDOW.0 & !WS_EX_LAYERED.0;
        SetWindowLongW(hwnd, GWL_EXSTYLE, ex as i32);

        let width = width.max(1);
        let height = height.max(1);

        let mut cursor = windows::Win32::Foundation::POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
        let work = info.rcWork;
        let anchor = anchor.unwrap_or(TrayIconAnchor {
            x: cursor.x,
            y: cursor.y,
            width: 16,
            height: 16,
        });
        let (x, y) = flyout_origin_in_work_area(
            (work.left, work.top, work.right, work.bottom),
            (anchor.x, anchor.y, anchor.width, anchor.height),
            width,
            height,
            extra_clearance,
        );

        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            x,
            y,
            width,
            height,
            SWP_SHOWWINDOW | SWP_FRAMECHANGED,
        );
        let _ = ShowWindow(hwnd, SW_SHOWNA);
        let _ = SetForegroundWindow(hwnd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_window_is_centered() {
        assert_eq!(cascade_delta(0), (0, 0));
        assert_eq!(cascade_origin(0, 0, 1000, 800, 480, 320, 0), (260, 240));
    }

    #[test]
    fn later_windows_step_down_and_right() {
        assert_eq!(cascade_delta(1), (CASCADE_STEP_PX, CASCADE_STEP_PX));
        assert_eq!(cascade_delta(2), (CASCADE_STEP_PX * 2, CASCADE_STEP_PX * 2));
        let (x0, y0) = cascade_origin(0, 0, 1000, 800, 480, 320, 0);
        let (x1, y1) = cascade_origin(0, 0, 1000, 800, 480, 320, 1);
        assert_eq!(x1 - x0, CASCADE_STEP_PX);
        assert_eq!(y1 - y0, CASCADE_STEP_PX);
    }

    #[test]
    fn cascade_stays_inside_work_area() {
        let (x, y) = cascade_origin(0, 0, 500, 400, 480, 320, 20);
        assert!(x >= 0 && x + 480 <= 500);
        assert!(y >= 0 && y + 320 <= 400);
    }

    #[test]
    fn flyout_sits_above_taskbar_icon_with_clearance() {
        // 1920×1080, 40px taskbar → rcWork bottom = 1040. Icon lives in the taskbar.
        let work = (0, 0, 1920, 1040);
        let icon = (1860, 1048, 24, 24);
        let (x, y) = flyout_origin_in_work_area(
            work,
            icon,
            FLYOUT_WIDTH_PX,
            FLYOUT_HEIGHT_PX,
            FLYOUT_TASKBAR_CLEARANCE_PX,
        );
        assert!(x >= 0 && x + FLYOUT_WIDTH_PX <= 1920);
        assert!(y >= 0);
        assert!(
            y + FLYOUT_HEIGHT_PX + FLYOUT_TASKBAR_CLEARANCE_PX <= 1040,
            "flyout must fully clear the taskbar, y={y}"
        );
        assert!(
            y + FLYOUT_HEIGHT_PX <= icon.1,
            "flyout must sit above the icon"
        );
    }

    #[test]
    fn flyout_stays_on_screen_at_left_edge() {
        let work = (0, 0, 800, 600);
        let icon = (8, 580, 24, 24);
        let (x, y) = flyout_origin_in_work_area(work, icon, 308, 400, 20);
        assert!(x >= 0);
        assert!(x + 308 <= 800);
        assert!(y >= 0);
        assert!(y + 400 + 20 <= 600);
    }
}
