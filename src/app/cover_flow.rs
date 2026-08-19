//! Background cover hydration. Cache hits never re-download.

use std::sync::Arc;

use gpui::{Context, RenderImage};

use super::LibraryApp;
use crate::covers::{
    extra_cover_cache_path, fetch_steam_cover_to_cache, fetch_url_cover_to_cache,
    render_image_from_path, resolve_local_cover_file,
};
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
                    !self.covers.contains_key(&game.id)
                        && !self.cover_inflight.contains(&game.id)
                        && (game.steam_app_id().is_some() || game.cover_url.is_some())
                })
                .cloned();
            let Some(game) = next else {
                break;
            };
            if let Some(path) =
                resolve_local_cover_file(&self.paths.root, &game, steam_root.as_deref())
            {
                if let Some(image) = render_image_from_path(&path) {
                    self.covers.insert(game.id.clone(), image);
                    continue;
                }
            }
            self.cover_inflight.insert(game.id.clone());
            if game.steam_app_id().is_some() {
                self.spawn_steam_cover_fetch(game, steam_root.clone(), cx);
            } else {
                self.spawn_url_cover_fetch(game, cx);
            }
        }
    }

    fn spawn_steam_cover_fetch(
        &mut self,
        game: LibraryTitle,
        steam_root: Option<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let Some(app_id) = game.steam_app_id() else {
            self.cover_inflight.remove(&game.id);
            return;
        };
        let root = self.paths.root.clone();
        let id = game.id.clone();
        let (tx, rx) = async_channel::bounded::<Option<Arc<RenderImage>>>(1);
        std::thread::spawn(move || {
            let image = fetch_steam_cover_to_cache(&root, steam_root.as_deref(), app_id)
                .ok()
                .and_then(|path| render_image_from_path(&path));
            let _ = tx.send_blocking(image);
        });
        self.await_cover(id, rx, cx);
    }

    fn spawn_url_cover_fetch(&mut self, game: LibraryTitle, cx: &mut Context<Self>) {
        let Some(url) = game.cover_url.clone() else {
            self.cover_inflight.remove(&game.id);
            return;
        };
        let dest = extra_cover_cache_path(&self.paths.root, &game.id);
        let id = game.id.clone();
        let (tx, rx) = async_channel::bounded::<Option<Arc<RenderImage>>>(1);
        std::thread::spawn(move || {
            let image = fetch_url_cover_to_cache(&dest, &url, true)
                .ok()
                .and_then(|path| render_image_from_path(&path));
            let _ = tx.send_blocking(image);
        });
        self.await_cover(id, rx, cx);
    }

    fn await_cover(
        &mut self,
        id: String,
        rx: async_channel::Receiver<Option<Arc<RenderImage>>>,
        cx: &mut Context<Self>,
    ) {
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
