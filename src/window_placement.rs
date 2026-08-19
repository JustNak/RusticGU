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
pub const FLYOUT_WIDTH_PX: i32 = 320;
pub const FLYOUT_HEIGHT_PX: i32 = 412;
/// Gap between the panel bottom and the tray-icon top.
pub const FLYOUT_ICON_GAP_PX: i32 = 12;
/// Extra inset from `rcWork` so a tall / auto-hide taskbar cannot clip the panel.
pub const FLYOUT_TASKBAR_CLEARANCE_PX: i32 = 16;
/// Used when `rcWork == rcMonitor` (auto-hide taskbar) so we still clear ~Win11 height.
pub const FLYOUT_TYPICAL_TASKBAR_PX: i32 = 48;

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
    logical_width: i32,
    logical_height: i32,
    extra_clearance: i32,
) {
    #[cfg(windows)]
    {
        if let Some(hwnd) = hwnd_of(window) {
            let scale = window.scale_factor().clamp(0.5, 4.0);
            place_flyout_hwnd(
                hwnd,
                anchor,
                logical_width,
                logical_height,
                scale,
                extra_clearance,
            );
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (
            window,
            anchor,
            logical_width,
            logical_height,
            extra_clearance,
        );
    }
}

/// Fallback origin used as the initial `window_bounds` so GPUI's first draw
/// has a real size (not a transparent 0-content popup).
///
/// Leaves a typical taskbar strip so the first paint is already above the bar
/// (the old first-display `height - 320` origin sat *in* the taskbar).
pub fn flyout_fallback_origin(cx: &App) -> Point<gpui::Pixels> {
    if let Some(display) = cx.displays().first() {
        let bounds = display.bounds();
        let taskbar = (FLYOUT_TYPICAL_TASKBAR_PX + FLYOUT_TASKBAR_CLEARANCE_PX) as f32;
        return point(
            bounds.origin.x + bounds.size.width
                - px((FLYOUT_WIDTH_PX + FLYOUT_TASKBAR_CLEARANCE_PX) as f32),
            bounds.origin.y + bounds.size.height - px(FLYOUT_HEIGHT_PX as f32) - px(taskbar),
        );
    }
    point(px(24.), px(24.))
}

/// Bottom inset reserved so the panel cannot overlap the taskbar.
///
/// When `rcWork` already excludes the bar, only `extra_clearance` is added.
/// When work equals the full monitor (auto-hide), reserve a typical taskbar
/// plus the extra gap.
pub fn flyout_bottom_clearance(
    monitor: (i32, i32, i32, i32),
    work: (i32, i32, i32, i32),
    extra_clearance: i32,
) -> i32 {
    let extra_clearance = extra_clearance.max(0);
    let measured = (monitor.3 - work.3).max(0);
    if measured == 0 {
        extra_clearance + FLYOUT_TYPICAL_TASKBAR_PX
    } else {
        extra_clearance
    }
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
    let monitor_bottom = work.3.max(anchor.1 + anchor.3);
    flyout_origin_above_icon(
        (work.0, work.1, work.2, monitor_bottom),
        work,
        anchor,
        flyout_w,
        flyout_h,
        FLYOUT_ICON_GAP_PX,
        extra_clearance,
    )
}

/// Icon-relative origin: right-aligned to the tray icon, above it + `icon_gap`,
/// then clamped so the panel fully clears the taskbar / work area.
pub fn flyout_origin_above_icon(
    monitor: (i32, i32, i32, i32),
    work: (i32, i32, i32, i32),
    anchor: (i32, i32, i32, i32),
    flyout_w: i32,
    flyout_h: i32,
    icon_gap: i32,
    extra_clearance: i32,
) -> (i32, i32) {
    let (work_left, work_top, work_right, work_bottom) = work;
    let (anchor_x, anchor_y, anchor_w, anchor_h) = anchor;
    let work_w = (work_right - work_left).max(0);
    let flyout_w = flyout_w.max(1);
    let flyout_h = flyout_h.max(1);
    let icon_gap = icon_gap.max(0);
    let edge = flyout_bottom_clearance(monitor, work, extra_clearance);

    let max_x = work_left + (work_w - flyout_w).max(0);
    let safe_bottom = work_bottom - edge;
    let max_y = work_top + (safe_bottom - work_top - flyout_h).max(0);

    // Right-align to the icon; notification-area icons sit at the right.
    let x = (anchor_x + anchor_w - flyout_w).clamp(work_left, max_x);

    // Prefer immediately above the icon. Icons usually live *in* the taskbar.
    let y_above_icon = anchor_y - icon_gap - flyout_h;
    let y_above_bar = safe_bottom - flyout_h;
    let mut y = y_above_icon.min(y_above_bar);
    if y < work_top {
        y = (anchor_y + anchor_h + icon_gap).min(max_y);
    }
    y = y.clamp(work_top, max_y);
    (x, y)
}

#[cfg(windows)]
fn place_flyout_hwnd(
    hwnd: windows::Win32::Foundation::HWND,
    anchor: Option<TrayIconAnchor>,
    logical_width: i32,
    logical_height: i32,
    scale: f32,
    extra_clearance: i32,
) {
    use windows::Win32::Foundation::{POINT, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetWindowLongW, GetWindowRect, SetForegroundWindow, SetWindowLongW,
        SetWindowPos, ShowWindow, GWL_EXSTYLE, GWL_STYLE, HWND_TOPMOST, SWP_FRAMECHANGED,
        SWP_SHOWWINDOW, SW_SHOWNA, WS_CAPTION, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_EX_APPWINDOW,
        WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_THICKFRAME, WS_VISIBLE,
    };

    unsafe {
        if hwnd.0.is_null() {
            return;
        }

        // gpui 0.2.2 PopUp windows are created with WINDOW_STYLE(0). Force a
        // caption-free popup so this is a panel, not a titled tool window.
        let style = (WS_POPUP | WS_VISIBLE | WS_CLIPCHILDREN | WS_CLIPSIBLINGS).0 as i32
            & !((WS_CAPTION | WS_THICKFRAME).0 as i32);
        SetWindowLongW(hwnd, GWL_STYLE, style);
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        let ex =
            (ex | WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0) & !WS_EX_APPWINDOW.0 & !WS_EX_LAYERED.0;
        SetWindowLongW(hwnd, GWL_EXSTYLE, ex as i32);
        apply_flyout_hwnd_chrome(hwnd);

        let want_w = ((logical_width.max(1) as f32) * scale).round() as i32;
        let want_h = ((logical_height.max(1) as f32) * scale).round() as i32;
        let (width, height) = flyout_physical_size(hwnd, want_w, want_h);

        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let anchor = anchor.unwrap_or(TrayIconAnchor {
            x: cursor.x,
            y: cursor.y,
            width: 16,
            height: 16,
        });
        let icon_rect = RECT {
            left: anchor.x,
            top: anchor.y,
            right: anchor.x + anchor.width.max(1),
            bottom: anchor.y + anchor.height.max(1),
        };
        let monitor = {
            let from_icon = MonitorFromRect(&icon_rect, MONITOR_DEFAULTTONEAREST);
            if from_icon.0.is_null() {
                MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST)
            } else {
                from_icon
            }
        };
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
        let work = info.rcWork;
        let screen = info.rcMonitor;
        let (x, y) = flyout_origin_above_icon(
            (screen.left, screen.top, screen.right, screen.bottom),
            (work.left, work.top, work.right, work.bottom),
            (anchor.x, anchor.y, anchor.width, anchor.height),
            width,
            height,
            FLYOUT_ICON_GAP_PX,
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

/// Prefer the HWND's current size when it already matches DPI-scaled logical size.
#[cfg(windows)]
fn flyout_physical_size(
    hwnd: windows::Win32::Foundation::HWND,
    want_w: i32,
    want_h: i32,
) -> (i32, i32) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

    let want_w = want_w.max(1);
    let want_h = want_h.max(1);
    let mut rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
        return (want_w, want_h);
    }
    let actual_w = rect.right - rect.left;
    let actual_h = rect.bottom - rect.top;
    if actual_w <= 0 || actual_h <= 0 {
        return (want_w, want_h);
    }
    let near = |actual: i32, want: i32| {
        let slack = (want.abs() / 4).max(8);
        (actual - want).abs() <= slack
    };
    if near(actual_w, want_w) && near(actual_h, want_h) {
        (actual_w, actual_h)
    } else {
        (want_w, want_h)
    }
}

/// Dark, rounded popup chrome (Win11 DWM). No-op on older builds.
#[cfg(windows)]
fn apply_flyout_hwnd_chrome(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
        DWMWCP_ROUND,
    };

    unsafe {
        let dark = BOOL::from(true);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark as *const _ as *const _,
            std::mem::size_of_val(&dark) as u32,
        );
        let corners = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corners as *const _ as *const _,
            std::mem::size_of_val(&corners) as u32,
        );
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
        let monitor = (0, 0, 1920, 1080);
        let work = (0, 0, 1920, 1040);
        let icon = (1860, 1048, 24, 24);
        let (x, y) = flyout_origin_above_icon(
            monitor,
            work,
            icon,
            FLYOUT_WIDTH_PX,
            FLYOUT_HEIGHT_PX,
            FLYOUT_ICON_GAP_PX,
            FLYOUT_TASKBAR_CLEARANCE_PX,
        );
        assert!(x >= 0 && x + FLYOUT_WIDTH_PX <= 1920);
        assert!(y >= 0);
        assert!(
            y + FLYOUT_HEIGHT_PX + FLYOUT_TASKBAR_CLEARANCE_PX <= 1040,
            "flyout must fully clear the taskbar, y={y}"
        );
        assert!(
            y + FLYOUT_HEIGHT_PX + FLYOUT_ICON_GAP_PX <= icon.1,
            "flyout must sit above the icon + gap"
        );
    }

    #[test]
    fn flyout_clears_tall_taskbar() {
        // 72px taskbar (large / tablet). Old first-display y = 1080-320 sits in the bar.
        let monitor = (0, 0, 1920, 1080);
        let work = (0, 0, 1920, 1008);
        let icon = (1860, 1020, 24, 24);
        let (x, y) = flyout_origin_above_icon(
            monitor,
            work,
            icon,
            FLYOUT_WIDTH_PX,
            FLYOUT_HEIGHT_PX,
            FLYOUT_ICON_GAP_PX,
            FLYOUT_TASKBAR_CLEARANCE_PX,
        );
        assert!(x + FLYOUT_WIDTH_PX <= 1920);
        assert!(
            y + FLYOUT_HEIGHT_PX + FLYOUT_TASKBAR_CLEARANCE_PX <= 1008,
            "must clear a 72px taskbar, y={y}"
        );
        assert!(y + FLYOUT_HEIGHT_PX <= icon.1);
        assert_ne!(
            y,
            1080 - 320,
            "must not use first-display height-320 origin"
        );
    }

    #[test]
    fn flyout_clears_autohide_taskbar_using_icon() {
        // Auto-hide: rcWork == rcMonitor. Sit above the icon and reserve a typical bar.
        let screen = (0, 0, 1920, 1080);
        let icon = (1860, 1048, 24, 24);
        let (x, y) = flyout_origin_above_icon(
            screen,
            screen,
            icon,
            FLYOUT_WIDTH_PX,
            FLYOUT_HEIGHT_PX,
            FLYOUT_ICON_GAP_PX,
            FLYOUT_TASKBAR_CLEARANCE_PX,
        );
        assert!(x + FLYOUT_WIDTH_PX <= 1920);
        assert!(
            y + FLYOUT_HEIGHT_PX + FLYOUT_TYPICAL_TASKBAR_PX + FLYOUT_TASKBAR_CLEARANCE_PX <= 1080,
            "auto-hide must still reserve a taskbar strip, y={y}"
        );
        assert!(y + FLYOUT_HEIGHT_PX <= icon.1);
    }

    #[test]
    fn flyout_stays_on_screen_at_left_edge() {
        let work = (0, 0, 800, 560);
        let icon = (8, 572, 24, 24);
        let (x, y) = flyout_origin_in_work_area(
            work,
            icon,
            FLYOUT_WIDTH_PX,
            FLYOUT_HEIGHT_PX,
            FLYOUT_TASKBAR_CLEARANCE_PX,
        );
        assert!(x >= 0);
        assert!(x + FLYOUT_WIDTH_PX <= 800);
        assert!(y >= 0);
        assert!(y + FLYOUT_HEIGHT_PX + FLYOUT_TASKBAR_CLEARANCE_PX <= 560);
    }

    #[test]
    fn flyout_right_aligns_to_icon() {
        let monitor = (0, 0, 1920, 1080);
        let work = (0, 0, 1920, 1040);
        let icon = (1700, 1048, 24, 24);
        let (x, _) = flyout_origin_above_icon(
            monitor,
            work,
            icon,
            FLYOUT_WIDTH_PX,
            FLYOUT_HEIGHT_PX,
            FLYOUT_ICON_GAP_PX,
            FLYOUT_TASKBAR_CLEARANCE_PX,
        );
        assert_eq!(x, 1700 + 24 - FLYOUT_WIDTH_PX);
    }
}
