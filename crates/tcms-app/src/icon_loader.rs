//! Shared icon downloader — avoids one thread + timer per list row.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use gtk4::Image;
use tcms_core::icons::{cached_icon_path_if_exists, ensure_cached_icon, is_remote_icon};
use tcms_core::Package;

#[derive(Clone)]
pub struct IconLoader {
    inner: Arc<Mutex<IconLoaderInner>>,
    runtime: Arc<tokio::runtime::Runtime>,
}

#[derive(Default)]
struct IconLoaderInner {
    pending: HashMap<String, Vec<Image>>,
    in_flight: HashSet<String>,
    ready: HashMap<String, PathBuf>,
}

impl IconLoader {
    pub fn new(runtime: Arc<tokio::runtime::Runtime>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(IconLoaderInner::default())),
            runtime,
        }
    }

    pub fn bind(&self, pkg: &Package, image: &Image, pixel_size: i32) {
        image.set_pixel_size(pixel_size);
        image.set_icon_name(Some("application-x-executable"));

        if let Some(name) = pkg.icon_name.as_deref() {
            if !is_remote_icon(name) {
                if std::path::Path::new(name).exists() {
                    image.set_from_file(Some(name));
                } else if !name.is_empty() {
                    image.set_icon_name(Some(name));
                }
            }
        }

        let Some(url) = pkg
            .icon_url
            .clone()
            .or_else(|| pkg.icon_name.clone().filter(|n| is_remote_icon(n)))
        else {
            return;
        };

        if let Some(path) = cached_icon_path_if_exists(&url) {
            image.set_from_file(Some(&path));
            if let Ok(mut guard) = self.inner.lock() {
                guard.ready.insert(url, path);
            }
            return;
        }

        {
            let mut guard = match self.inner.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            if let Some(path) = guard.ready.get(&url) {
                image.set_from_file(Some(path));
                return;
            }
            let start_download = !guard.in_flight.contains(&url);
            guard
                .pending
                .entry(url.clone())
                .or_default()
                .push(image.clone());
            if !start_download {
                return;
            }
            guard.in_flight.insert(url.clone());
        }

        let loader = self.clone();
        let runtime = self.runtime.clone();
        let url_for_thread = url.clone();
        let (tx, rx) = mpsc::channel();
        if std::thread::Builder::new()
            .name("tcms-icon".into())
            .spawn(move || {
                let path = runtime.block_on(ensure_cached_icon(&url_for_thread)).ok();
                let _ = tx.send((url_for_thread, path));
            })
            .is_err()
        {
            self.finish(&url, None);
            return;
        }

        glib::timeout_add_local(Duration::from_millis(100), move || match rx.try_recv() {
            Ok((url, path)) => {
                loader.finish(&url, path);
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(_) => glib::ControlFlow::Break,
        });
    }

    fn finish(&self, url: &str, path: Option<PathBuf>) {
        let waiters = {
            let mut guard = match self.inner.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            guard.in_flight.remove(url);
            if let Some(path) = path.clone() {
                guard.ready.insert(url.to_string(), path);
            }
            guard.pending.remove(url).unwrap_or_default()
        };
        if let Some(path) = path {
            for image in waiters {
                image.set_from_file(Some(&path));
            }
        }
    }
}
