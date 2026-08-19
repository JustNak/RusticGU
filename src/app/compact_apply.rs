//! Multi-title compact apply. Flow Maximum uses LZX for this action only.

use gpui::{Context, Window};

use super::compact_flow::{CompactFlowPhase, TitleCompactStats};
use super::LibraryApp;
use crate::compact::{
    apply_compact, apply_compact_allowing_lzx, decide_compact_apply, estimate_compact,
    measure_compact_sizes, CompactApplyDecision, CompactLevel, CompactOp, CompactProgress,
};
use crate::library::{title_is_compact_excluded, LibraryTitle};
use crate::notifications::notify_compact;

enum CompactJobMsg {
    Progress(CompactProgress),
    Stats(TitleCompactStats),
    Finished { ok_n: usize, fail_n: usize },
    Error(String),
}

impl LibraryApp {
    pub(crate) fn apply_compact_level(
        &mut self,
        level: CompactLevel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let titles = self
            .compact_flow
            .as_ref()
            .map(|flow| flow.titles.clone())
            .filter(|titles| !titles.is_empty())
            .unwrap_or_else(|| self.selected_titles());
        self.apply_compact_jobs(titles, CompactOp::Compress, Some(level), window, cx);
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
                self.dismiss_compact_flow_now(cx);
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
        let capture_stats = matches!(op, CompactOp::Compress);

        if let Some(flow) = self.compact_flow.as_mut() {
            flow.phase = CompactFlowPhase::Working;
            if let Some(level) = level {
                flow.selected_level = level;
            }
            flow.progress = Some(CompactProgress {
                processed: 0,
                total: runnable.len().max(1),
                message: "Starting…".into(),
            });
            flow.stats.clear();
            flow.failed = false;
            flow.finish_message.clear();
            flow.anim_gen = flow.anim_gen.saturating_add(1);
        }

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
        let (tx, rx) = async_channel::unbounded::<CompactJobMsg>();
        std::thread::spawn(move || {
            let mut ok_n = 0usize;
            let mut fail_n = 0usize;
            for (i, title) in runnable.into_iter().enumerate() {
                let _ = tx.send_blocking(CompactJobMsg::Progress(CompactProgress {
                    processed: i,
                    total,
                    message: format!("{} ({}/{})…", title.name, i + 1, total),
                }));
                let before = capture_stats.then(|| measure_compact_sizes(&title.install_path));
                let result = if allow_lzx {
                    apply_compact_allowing_lzx(
                        op,
                        &title.install_path,
                        algorithm,
                        allow,
                        |progress| {
                            let _ = tx.send_blocking(CompactJobMsg::Progress(CompactProgress {
                                processed: i,
                                total,
                                message: format!("{}: {}", title.name, progress.message),
                            }));
                        },
                    )
                } else {
                    apply_compact(op, &title.install_path, algorithm, allow, |progress| {
                        let _ = tx.send_blocking(CompactJobMsg::Progress(CompactProgress {
                            processed: i,
                            total,
                            message: format!("{}: {}", title.name, progress.message),
                        }));
                    })
                };
                match result {
                    Ok(_) => {
                        ok_n += 1;
                        if let Some(before) = before {
                            let after = measure_compact_sizes(&title.install_path);
                            let _ = tx.send_blocking(CompactJobMsg::Stats(TitleCompactStats {
                                id: title.id,
                                name: title.name,
                                before,
                                after,
                            }));
                        }
                    }
                    Err(err) => {
                        fail_n += 1;
                        let _ = tx
                            .send_blocking(CompactJobMsg::Error(format!("{}: {err}", title.name)));
                    }
                }
            }
            let _ = tx.send_blocking(CompactJobMsg::Finished { ok_n, fail_n });
        });

        let label = if names.len() == 1 {
            names[0].clone()
        } else {
            format!("{} titles", names.len())
        };
        cx.spawn(async move |this, cx| {
            while let Ok(item) = rx.recv().await {
                let cont = this.update(cx, |app, cx| match item {
                    CompactJobMsg::Progress(progress) => {
                        app.compact_progress = Some(progress.clone());
                        if let Some(flow) = app.compact_flow.as_mut() {
                            flow.progress = Some(progress);
                        }
                        cx.notify();
                        true
                    }
                    CompactJobMsg::Stats(stats) => {
                        if let Some(flow) = app.compact_flow.as_mut() {
                            flow.stats.push(stats);
                        }
                        cx.notify();
                        true
                    }
                    CompactJobMsg::Error(err) => {
                        if app.compact_flow.is_none() {
                            app.show_error_toast(err, cx);
                        } else if let Some(flow) = app.compact_flow.as_mut() {
                            flow.failed = true;
                            flow.finish_message = err;
                        }
                        cx.notify();
                        true
                    }
                    CompactJobMsg::Finished { ok_n, fail_n } => {
                        app.finish_compact_job(op, &label, total, ok_n, fail_n, cx);
                        false
                    }
                });
                if !matches!(cont, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    fn finish_compact_job(
        &mut self,
        op: CompactOp,
        label: &str,
        total: usize,
        ok_n: usize,
        fail_n: usize,
        cx: &mut Context<Self>,
    ) {
        self.compact_busy = false;
        self.live.set_compact_busy(false);
        let verb = match op {
            CompactOp::Compress => "Compressed",
            CompactOp::Uncompress => "Restored",
        };
        let message = format!("{verb} {ok_n}/{total} with WOF /EXE. Failed {fail_n}.");
        let failed = fail_n > 0;
        self.compact_progress = Some(CompactProgress {
            processed: total,
            total,
            message: message.clone(),
        });
        if let Some(flow) = self.compact_flow.as_mut() {
            flow.phase = CompactFlowPhase::Done;
            flow.failed = failed || flow.failed;
            flow.finish_message = message.clone();
            flow.progress = None;
            flow.anim_gen = flow.anim_gen.saturating_add(1);
        } else if failed {
            self.show_error_toast(message.clone(), cx);
        } else {
            self.show_toast(message.clone(), cx);
        }
        notify_compact(
            self.system_tray.as_ref(),
            self.settings.os_notify_mode,
            self.window_hidden_to_tray,
            crate::branding::APP_NAME,
            &format!("{label}: {message}"),
            !failed,
        );
        self.refresh_library(cx);
        cx.notify();
    }
}
