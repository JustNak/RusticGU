//! Multi-title compact apply. Flow Maximum uses LZX for this action only.

use gpui::{Context, Window};

use super::compact_flow::{compact_job_summary, CompactFlowPhase, TitleCompactStats};
use super::LibraryApp;
use crate::compact::{
    apply_compact, apply_compact_allowing_lzx, apply_compact_force, decide_compact_apply,
    estimate_compact, measure_compact_sizes, CompactApplyDecision, CompactLevel, CompactOp,
    CompactProgress,
};
use crate::library::{title_is_compact_excluded, LibraryTitle};
use crate::live::LiveHandle;
use crate::notifications::notify_compact;

enum CompactJobMsg {
    Progress {
        title_id: String,
        progress: CompactProgress,
    },
    Stats(TitleCompactStats),
    Finished {
        ok_n: usize,
        fail_n: usize,
    },
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PosterJobKind {
    Compress,
    Decompress,
    Change,
    Walkback,
}

#[derive(Debug, Clone)]
pub(crate) struct PosterJob {
    pub title_ids: Vec<String>,
    pub current_id: String,
    pub kind: PosterJobKind,
    pub progress: CompactProgress,
}

impl PosterJob {
    pub(crate) fn for_titles(
        titles: &[LibraryTitle],
        kind: PosterJobKind,
        message: impl Into<String>,
    ) -> Option<Self> {
        let title_ids: Vec<String> = titles.iter().map(|t| t.id.clone()).collect();
        let current_id = title_ids.first()?.clone();
        Some(Self {
            title_ids,
            current_id,
            kind,
            progress: CompactProgress {
                processed: 0,
                total: 1,
                message: message.into(),
            },
        })
    }

    pub(crate) fn covers(&self, id: &str) -> bool {
        self.title_ids.iter().any(|t| t == id)
    }

    pub(crate) fn waiting(&self, id: &str) -> bool {
        self.covers(id) && self.current_id != id
    }
}

/// What the poster and inspector should show for one title.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TitleActivity {
    Idle,
    Excluded,
    Patching,
    Job {
        kind: PosterJobKind,
        waiting: bool,
        progress: CompactProgress,
    },
}

impl TitleActivity {
    pub(crate) fn resolve(
        title: &LibraryTitle,
        job: Option<&PosterJob>,
        live: &LiveHandle,
    ) -> Self {
        if let Some(job) = job.filter(|job| job.covers(&title.id)) {
            return Self::Job {
                kind: job.kind,
                waiting: job.waiting(&title.id),
                progress: job.progress.clone(),
            };
        }
        if title
            .steam_app_id()
            .is_some_and(|id| live.is_locked(&id.to_string()))
        {
            return Self::Patching;
        }
        if title_is_compact_excluded(title) {
            return Self::Excluded;
        }
        Self::Idle
    }

    pub(crate) fn heading(&self) -> Option<&'static str> {
        match self {
            Self::Idle => None,
            Self::Excluded => Some("Excluded"),
            Self::Patching => Some("Patching"),
            Self::Job { waiting: true, .. } => Some("Waiting…"),
            Self::Job {
                kind: PosterJobKind::Decompress,
                ..
            } => Some("Restoring"),
            Self::Job {
                kind: PosterJobKind::Change,
                ..
            } => Some("Changing"),
            Self::Job {
                kind: PosterJobKind::Compress,
                ..
            } => Some("Compressing"),
            Self::Job {
                kind: PosterJobKind::Walkback,
                ..
            } => Some("Walking back"),
        }
    }

    pub(crate) fn detail(&self) -> Option<String> {
        match self {
            Self::Idle => None,
            Self::Excluded => Some("Excluded from compact".into()),
            Self::Patching => {
                Some("Steam is updating this title. Compact waits until the patch finishes.".into())
            }
            Self::Job { waiting: true, .. } => Some("Queued behind another title.".into()),
            Self::Job { progress, .. } => Some(progress.message.clone()),
        }
    }

    pub(crate) fn percent(&self) -> Option<f32> {
        let Self::Job {
            kind: _,
            waiting,
            progress,
        } = self
        else {
            return None;
        };
        if *waiting || progress.total == 0 {
            return Some(0.0);
        }
        let pct = (progress.processed as f32 / progress.total as f32) * 100.0;
        Some(pct.clamp(0.0, 100.0))
    }

    pub(crate) fn allows_compact(&self) -> bool {
        matches!(self, Self::Idle)
    }
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
        self.apply_compact_jobs(titles, CompactOp::Compress, Some(level), false, window, cx);
    }

    pub(crate) fn apply_change_level(
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
        self.apply_compact_jobs(titles, CompactOp::Compress, Some(level), true, window, cx);
    }

    pub(crate) fn apply_compact_jobs(
        &mut self,
        titles: Vec<LibraryTitle>,
        op: CompactOp,
        level: Option<CompactLevel>,
        force: bool,
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
        let poster_kind = match (op, force) {
            (CompactOp::Uncompress, _) => PosterJobKind::Decompress,
            (CompactOp::Compress, true) => PosterJobKind::Change,
            (CompactOp::Compress, false) => PosterJobKind::Compress,
        };
        let use_theater =
            self.compact_flow.is_some() && matches!(poster_kind, PosterJobKind::Compress);

        if use_theater {
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
            self.poster_job = None;
        } else {
            self.dismiss_compact_flow_now(cx);
            self.poster_job = PosterJob::for_titles(
                &runnable,
                poster_kind,
                match poster_kind {
                    PosterJobKind::Decompress => "Restoring…",
                    PosterJobKind::Change => "Changing…",
                    PosterJobKind::Compress => "Compressing…",
                    PosterJobKind::Walkback => "Walking back…",
                },
            );
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
                let title_id = title.id.clone();
                let _ = tx.send_blocking(CompactJobMsg::Progress {
                    title_id: title_id.clone(),
                    progress: CompactProgress {
                        processed: 0,
                        total: 1,
                        message: format!("{} ({}/{})…", title.name, i + 1, total),
                    },
                });
                let before = capture_stats.then(|| measure_compact_sizes(&title.install_path));
                let on_progress = |progress: CompactProgress| {
                    let _ = tx.send_blocking(CompactJobMsg::Progress {
                        title_id: title_id.clone(),
                        progress: CompactProgress {
                            processed: progress.processed,
                            total: progress.total.max(1),
                            message: format!("{}: {}", title.name, progress.message),
                        },
                    });
                };
                let result = if force {
                    apply_compact_force(op, &title.install_path, algorithm, allow, on_progress)
                } else if allow_lzx {
                    apply_compact_allowing_lzx(
                        op,
                        &title.install_path,
                        algorithm,
                        allow,
                        on_progress,
                    )
                } else {
                    apply_compact(op, &title.install_path, algorithm, allow, on_progress)
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
                    CompactJobMsg::Progress { title_id, progress } => {
                        app.compact_progress = Some(progress.clone());
                        if let Some(job) = app.poster_job.as_mut() {
                            job.current_id = title_id;
                            job.progress = progress.clone();
                        }
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
        let message = compact_job_summary(op, label, ok_n, fail_n, total);
        let failed = fail_n > 0;
        self.poster_job = None;
        self.compact_progress = None;
        if let Some(flow) = self.compact_flow.as_mut() {
            let had_err_detail = flow.failed && !flow.finish_message.is_empty();
            flow.phase = CompactFlowPhase::Done;
            flow.failed = failed || flow.failed;
            if !flow.failed {
                flow.finish_message = format!("Finished with {}.", flow.selected_level.label());
            } else if !had_err_detail {
                flow.finish_message = message.clone();
            }
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

#[cfg(test)]
mod tests {
    use super::{PosterJob, PosterJobKind, TitleActivity};
    use crate::compact::CompactProgress;
    use crate::library::{LibraryStore, LibraryTitle};
    use crate::live::LiveHandle;
    use std::path::PathBuf;

    fn steam_title(app_id: u32, name: &str) -> LibraryTitle {
        LibraryTitle {
            id: format!("steam:{app_id}"),
            name: name.into(),
            install_path: PathBuf::from(r"D:\Steam\steamapps\common").join(name),
            store: LibraryStore::Steam,
            launcher_id: Some(app_id.to_string()),
            last_played_unix: None,
            logical_bytes: Some(100),
            on_disk_bytes: Some(40),
            compacted: true,
            steam_app_id: Some(app_id),
            steam_library_path: None,
            steam_install_dir_name: None,
            cover_url: None,
        }
    }

    #[test]
    fn decompress_job_beats_patching_and_shows_restoring() {
        let title = steam_title(1, "Bloons");
        let live = LiveHandle::for_tests();
        live.lock_title("1");
        let job = PosterJob {
            title_ids: vec![title.id.clone()],
            current_id: title.id.clone(),
            kind: PosterJobKind::Decompress,
            progress: CompactProgress {
                processed: 40,
                total: 400,
                message: "Starting WOF uncompact…".into(),
            },
        };
        let activity = TitleActivity::resolve(&title, Some(&job), &live);
        assert_eq!(activity.heading(), Some("Restoring"));
        assert!(activity.percent().unwrap() > 0.0);
        assert!(!activity.allows_compact());
    }

    #[test]
    fn locked_steam_title_is_patching() {
        let title = steam_title(440, "Bloons");
        let live = LiveHandle::for_tests();
        live.lock_title("440");
        let activity = TitleActivity::resolve(&title, None, &live);
        assert_eq!(activity, TitleActivity::Patching);
        assert_eq!(activity.heading(), Some("Patching"));
        assert!(!activity.allows_compact());
        assert!(activity.percent().is_none());
    }

    #[test]
    fn queued_title_waits() {
        let title = steam_title(2, "Celeste");
        let live = LiveHandle::for_tests();
        let job = PosterJob {
            title_ids: vec!["steam:1".into(), title.id.clone()],
            current_id: "steam:1".into(),
            kind: PosterJobKind::Compress,
            progress: CompactProgress {
                processed: 10,
                total: 20,
                message: "working".into(),
            },
        };
        let activity = TitleActivity::resolve(&title, Some(&job), &live);
        assert_eq!(activity.heading(), Some("Waiting…"));
        assert_eq!(activity.percent(), Some(0.0));
    }
}
