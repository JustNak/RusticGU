#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod activate;
mod app;
mod appearance;
mod assets;
mod branding;
mod compact;
mod covers;
mod format;
mod library;
mod live;
mod notifications;
mod persistence;
mod settings;
mod single_instance;
mod startup;
mod tray;
mod updater;
mod window_icon;
mod window_placement;

use activate::{start_activate_server, ActivateBridge};
use app::LibraryApp;
use assets::Assets;
use branding::APP_NAME;
#[cfg(windows)]
use branding::APP_USER_MODEL_ID;
use gpui::{
    point, px, size, App, AppContext, Application, Bounds, SharedString, WindowBounds,
    WindowDecorations, WindowOptions,
};
use gpui_component::{Root, TitleBar};
use persistence::{app_paths, ensure_app_dirs, load_settings};
use settings::{WindowLayout, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};
use single_instance::{claim_instance, InstanceRole};
use startup::{apply_launch_at_startup, launched_minimized};
use window_icon::apply_app_icon;
use window_placement::center_window;

fn main() {
    set_app_user_model_id();

    if claim_instance() == InstanceRole::Secondary {
        return;
    }

    let paths = app_paths();
    let _ = ensure_app_dirs(&paths);
    let settings = load_settings(&paths);
    let _ = apply_launch_at_startup(settings.launch_at_startup, settings.startup_minimized);
    let start_hidden = launched_minimized();

    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let _guard = runtime.enter();

    let activate = ActivateBridge::new();
    start_activate_server(activate.clone());

    let initial_settings = settings;
    let initial_paths = paths;
    let initial_activate = activate;

    Application::new()
        .with_assets(Assets::new())
        .run(move |cx: &mut App| {
            gpui_component::init(cx);

            let (window_bounds, needs_center) =
                window_bounds_from_layout(&initial_settings.window_layout, cx);
            let settings = initial_settings;
            let paths = initial_paths;
            let activate = initial_activate.clone();

            cx.spawn(async move |cx| {
                let handle = cx
                    .open_window(
                        WindowOptions {
                            window_bounds: Some(window_bounds),
                            titlebar: Some({
                                let mut opts = TitleBar::title_bar_options();
                                opts.title = Some(SharedString::from(APP_NAME));
                                opts
                            }),
                            window_decorations: Some(WindowDecorations::Client),
                            window_min_size: Some(size(
                                px(MIN_WINDOW_WIDTH),
                                px(MIN_WINDOW_HEIGHT),
                            )),
                            show: !start_hidden,
                            ..Default::default()
                        },
                        move |window, cx| {
                            apply_app_icon(window);
                            let view =
                                cx.new(|cx| LibraryApp::new(settings, paths, activate, window, cx));
                            cx.new(|cx| Root::new(view, window, cx))
                        },
                    )
                    .expect("open window");

                if needs_center {
                    let _ = handle.update(cx, |_root, window, _cx| {
                        center_window(window);
                    });
                }
            })
            .detach();
        });
}

fn window_bounds_from_layout(layout: &WindowLayout, cx: &App) -> (WindowBounds, bool) {
    let mut layout = layout.clone();
    layout.sanitize();

    let size = size(px(layout.width), px(layout.height));
    let (bounds, needs_center) = match (layout.x, layout.y) {
        (Some(x), Some(y)) => {
            let candidate = Bounds {
                origin: point(px(x), px(y)),
                size,
            };
            if bounds_visible_on_any_display(&candidate, cx) {
                (candidate, false)
            } else {
                (Bounds::centered(None, size, cx), true)
            }
        }
        _ => (Bounds::centered(None, size, cx), true),
    };

    let window_bounds = if layout.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    };

    (window_bounds, needs_center && !layout.maximized)
}

fn bounds_visible_on_any_display(bounds: &Bounds<gpui::Pixels>, cx: &App) -> bool {
    cx.displays()
        .iter()
        .any(|display| display.bounds().intersects(bounds))
}

fn set_app_user_model_id() {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

        let wide: Vec<u16> = std::ffi::OsStr::new(APP_USER_MODEL_ID)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let _ = unsafe { SetCurrentProcessExplicitAppUserModelID(PCWSTR(wide.as_ptr())) };
    }
}
