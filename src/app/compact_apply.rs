//! Multi-title compact apply. Dialog High uses LZX for this action only.

use gpui::{Context, Window};

use super::LibraryApp;
use crate::compact::{
    apply_compact, apply_compact_allowing_lzx, decide_compact_apply, estimate_compact,
    CompactApplyDecision, CompactLevel, CompactOp, CompactProgress,
};
use crate::library::{title_is_compact_excluded, LibraryTitle};
use crate::notifications::notify_compact;

impl LibraryApp {
    pub(crate) fn apply_compact_level(
        &mut self,
        level: CompactLevel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_compact_jobs(
            self.selected_titles(),
            CompactOp::Compress,
            Some(level),
            window,
            cx,
        );
    }

    pub(crate) fn apply_compact_jobs(
        &mut self,
        titles: Vec<LibraryTitle>,
        op: CompactOp,
        level: Option<CompactLevel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.compact_busy {
            self.show_toast("A compact job is already running.", cx);
            return;
        }
        if titles.is_empty() {
            self.show_toast("Select a game first.", cx);
            return;
        }

        let allow = self.settings.allow_dstorage_override;
        let mut runnable = Vec::new();
        let mut skipped_excluded = Vec::new();
        let mut skipped_dstorage = Vec::new();
        let mut refused = Vec::new();

        for title in titles {
            if title_is_compact_excluded(&title) {
                skipped_excluded.push(title.name);
                continue;
            }
            if let Some(app_id) = title.steam_app_id() {
                if self.live.is_locked(&app_id.to_string()) {
                    refused.push(format!("{} is patching.", title.name));
                    continue;
                }
            }
            match decide_compact_apply(&title.install_path, allow) {
                CompactApplyDecision::Apply => runnable.push(title),
                CompactApplyDecision::SkipDirectStorage => skipped_dstorage.push(title),
                CompactApplyDecision::Refuse(msg) => refused.push(format!("{}: {msg}", title.name)),
            }
        }

        if runnable.is_empty() && skipped_dstorage.len() == 1 && skipped_excluded.is_empty() {
            let title = &skipped_dstorage[0];
            if let Ok(estimate) = estimate_compact(
                &title.install_path,
                self.settings.compact_algorithm.for_live_library(),
            ) {
                self.open_dstorage_warning(window, cx, estimate);
            } else {
                self.show_error_toast(
                    "dstorage.dll or dstoragecore.dll is present. Enable the override in Settings → General to compact.",
                    cx,
                );
            }
            return;
        }

        if !skipped_dstorage.is_empty() {
            let names = skipped_dstorage
                .iter()
                .map(|t| t.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            self.show_toast(
                format!("Skipped DirectStorage titles (dstorage.dll): {names}"),
                cx,
            );
        }
        if !skipped_excluded.is_empty() {
            self.show_toast(
                format!("Skipped excluded titles: {}", skipped_excluded.join(", ")),
                cx,
            );
        }
        if !refused.is_empty() {
            self.show_error_toast(refused.join(" "), cx);
        }
        if runnable.is_empty() {
            return;
        }

        let algorithm = match (op, level) {
            (CompactOp::Compress, Some(level)) => level.algorithm(),
            _ => self.settings.compact_algorithm.for_live_library(),
        };
        let allow_lzx = matches!(level, Some(level) if level.allows_lzx())
            || algorithm == crate::settings::CompactAlgorithm::Lzx;

        self.compact_busy = true;
        self.live.set_compact_busy(true);
        self.compact_progress = Some(CompactProgress {
            processed: 0,
            total: runnable.len().max(1),
            message: "Starting…".into(),
        });
        cx.notify();

        let total = runnable.len();
        let names: Vec<String> = runnable.iter().map(|t| t.name.clone()).collect();
        let (tx, rx) = async_channel::unbounded::<Result<CompactProgress, String>>();
        std::thread::spawn(move || {
            let mut ok_n = 0usize;
            let mut fail_n = 0usize;
            for (i, title) in runnable.into_iter().enumerate() {
                let _ = tx.send_blocking(Ok(CompactProgress {
                    processed: i,
                    total,
                    message: format!("{} ({}/{})…", title.name, i + 1, total),
                }));
                let result = if allow_lzx {
                    apply_compact_allowing_lzx(
                        op,
                        &title.install_path,
                        algorithm,
                        allow,
                        |progress| {
                            let _ = tx.send_blocking(Ok(CompactProgress {
                                processed: i,
                                total,
                                message: format!("{}: {}", title.name, progress.message),
                            }));
                        },
                    )
                } else {
                    apply_compact(op, &title.install_path, algorithm, allow, |progress| {
                        let _ = tx.send_blocking(Ok(CompactProgress {
                            processed: i,
                            total,
                            message: format!("{}: {}", title.name, progress.message),
                        }));
                    })
                };
                match result {
                    Ok(_) => ok_n += 1,
                    Err(err) => {
                        fail_n += 1;
                        let _ = tx.send_blocking(Err(format!("{}: {err}", title.name)));
                    }
                }
            }
            let verb = match op {
                CompactOp::Compress => "Compressed",
                CompactOp::Uncompress => "Restored",
            };
            let _ = tx.send_blocking(Ok(CompactProgress {
                processed: total,
                total,
                message: format!("{verb} {ok_n}/{total} with WOF /EXE. Failed {fail_n}."),
            }));
        });

        let label = if names.len() == 1 {
            names[0].clone()
        } else {
            format!("{} titles", names.len())
        };
        cx.spawn(async move |this, cx| {
            while let Ok(item) = rx.recv().await {
                let cont = this.update(cx, |app, cx| match item {
                    Ok(progress) => {
                        let finished = progress.processed >= progress.total
                            && (progress.message.contains("WOF /EXE")
                                || progress.message.starts_with("Compressed ")
                                || progress.message.starts_with("Restored "));
                        app.compact_progress = Some(progress.clone());
                        if finished {
                            app.compact_busy = false;
                            app.live.set_compact_busy(false);
                            let failed = progress.message.contains("Failed ")
                                && !progress.message.contains("Failed 0");
                            if failed {
                                app.show_error_toast(progress.message.clone(), cx);
                            } else {
                                app.show_toast(progress.message.clone(), cx);
                            }
                            notify_compact(
                                app.system_tray.as_ref(),
                                app.settings.os_notify_mode,
                                app.window_hidden_to_tray,
                                crate::branding::APP_NAME,
                                &format!("{label}: {}", progress.message),
                                !failed,
                            );
                            app.refresh_library(cx);
                        }
                        cx.notify();
                        true
                    }
                    Err(err) => {
                        app.show_error_toast(err, cx);
                        cx.notify();
                        true
                    }
                });
                if !matches!(cont, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }
}
