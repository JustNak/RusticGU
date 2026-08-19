//! Background cover hydration. Cache hits never re-download.

use std::sync::Arc;

use gpui::{Context, RenderImage};

use super::LibraryApp;
use crate::covers::{fetch_steam_cover_to_cache, render_image_from_path, resolve_local_cover_file};
use crate::library::{steam_path, LibraryTitle};

impl LibraryApp {
    pub(crate) fn hydrate_covers_from_disk(&mut self) {
        let steam_root = steam_path();
        for game in &self.games {
            if self.covers.contains_key(&game.id) {
                continue;
            }
            if let Some(path) =
                resolve_local_cover_file(&self.paths.root, game, steam_root.as_deref())
            {
                if let Some(image) = render_image_from_path(&path) {
                    self.covers.insert(game.id.clone(), image);
                }
            }
        }
    }

    pub(crate) fn request_covers(&mut self, cx: &mut Context<Self>) {
        const MAX_INFLIGHT: usize = 4;
        let steam_root = steam_path();
        while self.cover_inflight.len() < MAX_INFLIGHT {
            let next = self
                .games
                .iter()
                .find(|game| {
                    game.steam_app_id().is_some()
                        && !self.covers.contains_key(&game.id)
                        && !self.cover_inflight.contains(&game.id)
                })
                .cloned();
            let Some(game) = next else {
                break;
            };
            // Local steam library cache may have appeared after scan.
            if let Some(path) =
                resolve_local_cover_file(&self.paths.root, &game, steam_root.as_deref())
            {
                if let Some(image) = render_image_from_path(&path) {
                    self.covers.insert(game.id.clone(), image);
                    continue;
                }
            }
            self.cover_inflight.insert(game.id.clone());
            self.spawn_steam_cover_fetch(game, cx);
        }
    }

    fn spawn_steam_cover_fetch(&mut self, game: LibraryTitle, cx: &mut Context<Self>) {
        let Some(app_id) = game.steam_app_id() else {
            self.cover_inflight.remove(&game.id);
            return;
        };
        let root = self.paths.root.clone();
        let id = game.id.clone();
        let (tx, rx) = async_channel::bounded::<Option<Arc<RenderImage>>>(1);
        std::thread::spawn(move || {
            let image = fetch_steam_cover_to_cache(&root, app_id)
                .ok()
                .and_then(|path| render_image_from_path(&path));
            let _ = tx.send_blocking(image);
        });
        cx.spawn(async move |this, cx| {
            if let Ok(image) = rx.recv().await {
                let _ = this.update(cx, |app, cx| {
                    app.cover_inflight.remove(&id);
                    if let Some(image) = image {
                        app.covers.insert(id, image);
                    }
                    app.request_covers(cx);
                    cx.notify();
                });
            }
        })
        .detach();
    }
}
