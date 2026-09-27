use std::rc::Rc;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use gtk4::prelude::*;
use tcms_aur::AurBackend;
use tcms_core::transactions::{
    update_backends, PackageListing, UpdateReport, PACKAGE_TRANSACTIONS,
};
use tcms_core::{
    fetch_flathub_collection, packages_match, search_text_for, AppConfig, Backend, FeaturedSection,
    InstallState, Package, PackageAction, PackageId, PackageKind, PackageSource, SearchQuery,
};
use tcms_flatpak::FlatpakBackend;
use tcms_pacman::PacmanBackend;

#[derive(Clone, Copy)]
pub enum ListKind {
    Explore,
    Installed,
}

#[derive(Clone)]
pub struct StoreService {
    inner: Arc<std::sync::Mutex<StoreInner>>,
    runtime: Arc<tokio::runtime::Runtime>,
    installed_cache: Arc<tcms_core::cache::SnapshotCache<PackageListing>>,
}

struct StoreInner {
    config: AppConfig,
    pacman: PacmanBackend,
    flatpak: FlatpakBackend,
    aur: AurBackend,
}

impl StoreService {
    pub fn new() -> Self {
        let config = AppConfig::load().unwrap_or_default();
        let pacman = PacmanBackend::new(
            config.enable_pacman,
            config.advanced.pacman_conf.clone(),
            config.advanced.pacman_extra_args.clone(),
        );
        let flatpak = FlatpakBackend::new(
            config.enable_flatpak,
            config.advanced.flatpak_installation.clone(),
            &config.advanced.flatpak_remotes,
        );
        let aur = AurBackend::new(
            config.enable_aur,
            config.advanced.aur_rpc_url.clone(),
            config.advanced.aur_helper.clone(),
            config.advanced.aur_extra_args.clone(),
        );
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("tcms-worker")
            .build()
            .expect("tokio runtime");
        Self {
            inner: Arc::new(std::sync::Mutex::new(StoreInner {
                config,
                pacman,
                flatpak,
                aur,
            })),
            runtime: Arc::new(runtime),
            installed_cache: Arc::new(tcms_core::cache::SnapshotCache::new(Duration::from_secs(
                30,
            ))),
        }
    }

    pub fn runtime(&self) -> Arc<tokio::runtime::Runtime> {
        self.runtime.clone()
    }

    fn with_inner<R>(&self, f: impl FnOnce(&StoreInner) -> R) -> R {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        f(&guard)
    }

    fn with_inner_mut<R>(&self, f: impl FnOnce(&mut StoreInner) -> R) -> R {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut guard)
    }

    fn backends(&self) -> (PacmanBackend, FlatpakBackend, AurBackend) {
        self.with_inner(|inner| {
            (
                inner.pacman.clone(),
                inner.flatpak.clone(),
                inner.aur.clone(),
            )
        })
    }

    pub fn config(&self) -> AppConfig {
        self.with_inner(|inner| inner.config.clone())
    }

    pub fn save_config(&self, config: AppConfig) -> tcms_core::Result<()> {
        self.with_inner_mut(|inner| {
            config.save()?;
            inner.pacman.set_enabled(config.enable_pacman);
            inner
                .pacman
                .set_pacman_conf(config.advanced.pacman_conf.clone());
            inner
                .pacman
                .set_extra_args(config.advanced.pacman_extra_args.clone());
            inner.flatpak.set_enabled(config.enable_flatpak);
            inner
                .flatpak
                .set_installation(config.advanced.flatpak_installation.clone());
            inner
                .flatpak
                .set_remotes_from_text(&config.advanced.flatpak_remotes);
            inner.aur.set_enabled(config.enable_aur);
            inner.aur.set_rpc_url(config.advanced.aur_rpc_url.clone());
            inner.aur.set_helper(config.advanced.aur_helper.clone());
            inner
                .aur
                .set_extra_args(config.advanced.aur_extra_args.clone());
            inner.config = config;
            self.installed_cache.invalidate();
            Ok(())
        })
    }

    fn filter_catalog(&self, mut packages: Vec<Package>) -> Vec<Package> {
        let config = self.config();
        packages.retain(|p| config.allows_package(p));
        packages
    }

    pub fn explore(&self, text: &str) -> PackageListing {
        if text.trim().is_empty() {
            return PackageListing::default();
        }
        let mut listing = self.search(SearchQuery {
            text: text.to_string(),
            ..Default::default()
        });
        listing.packages = self.filter_catalog(listing.packages);
        listing
            .errors
            .extend(self.annotate_install_states(&mut listing.packages));
        let priority = self.config().advanced.clone();
        listing
            .packages
            .sort_by_key(|p| (priority.priority_rank(p.id.source), p.name.to_lowercase()));
        listing
    }

    /// Rich package details for the detail page, plus alternate sources.
    pub fn package_details(&self, pkg: &Package) -> (Package, Vec<Package>) {
        let mut detailed = self.enrich_one(pkg);
        let mut alts = self.resolve_install_candidates_light(pkg);
        alts.retain(|p| p.id != detailed.id);
        self.annotate_install_states(std::slice::from_mut(&mut detailed));
        self.annotate_install_states(&mut alts);
        (detailed, alts)
    }

    fn enrich_one(&self, pkg: &Package) -> Package {
        let (pacman, flatpak, aur) = self.backends();
        let id = pkg.id.clone();
        let mut detailed = self
            .runtime
            .block_on(async {
                match id.source {
                    PackageSource::Pacman if pacman.enabled() => {
                        pacman.get_package(&id).await.ok().flatten()
                    }
                    PackageSource::Flatpak if flatpak.enabled() => {
                        flatpak.get_package(&id).await.ok().flatten()
                    }
                    PackageSource::Aur if aur.enabled() => {
                        aur.get_package(&id).await.ok().flatten()
                    }
                    _ => None,
                }
            })
            .unwrap_or_else(|| pkg.clone());

        // Keep the list entry's nicer display name / icon when backends return stubs.
        if !pkg.name.is_empty()
            && pkg.name != detailed.id.id
            && (detailed.name.is_empty()
                || detailed.name == detailed.id.id
                || pkg.name.chars().any(|c| c.is_uppercase()))
        {
            detailed.name = pkg.name.clone();
        }
        if let Some(icon) = pkg.icon_name.as_ref() {
            let generic = matches!(
                detailed.icon_name.as_deref(),
                None | Some("package-x-generic") | Some("application-x-executable")
            );
            if generic {
                detailed.icon_name = Some(icon.clone());
            }
        }
        if detailed.icon_url.is_none() {
            detailed.icon_url = pkg.icon_url.clone();
        }
        if detailed.summary.is_empty() {
            detailed.summary = pkg.summary.clone();
        }
        if detailed.description.is_empty() {
            detailed.description = pkg.description.clone();
        }
        // Prefer richer metadata from either side.
        if detailed.publisher.is_none() {
            detailed.publisher = pkg.publisher.clone();
        }
        if detailed.developer.is_none() {
            detailed.developer = pkg.developer.clone();
        }
        if detailed.license.is_none() {
            detailed.license = pkg.license.clone();
        }
        if detailed.homepage.is_none() {
            detailed.homepage = pkg.homepage.clone();
        }
        if detailed.bug_url.is_none() {
            detailed.bug_url = pkg.bug_url.clone();
        }
        if detailed.donate_url.is_none() {
            detailed.donate_url = pkg.donate_url.clone();
        }
        if detailed.size_bytes.is_none() {
            detailed.size_bytes = pkg.size_bytes;
        }
        if detailed.is_proprietary.is_none() {
            detailed.is_proprietary = pkg.is_proprietary;
        }
        if matches!(pkg.state, InstallState::Installed | InstallState::Updatable) {
            detailed.state = pkg.state;
            if detailed.available_version.is_none() {
                detailed.available_version = pkg.available_version.clone();
            }
        }
        detailed.apply_license_heuristics();
        detailed
    }

    pub fn package_details_async<F>(&self, pkg: Package, on_done: F)
    where
        F: FnOnce(Package, Vec<Package>) + 'static,
    {
        let store = self.clone();
        let (tx, rx) = mpsc::channel();
        let pkg_fallback = pkg.clone();
        if !spawn_named("tcms-details", move || {
            let (detailed, alts) = store.package_details(&pkg);
            let _ = tx.send((detailed, alts));
        }) {
            on_done(pkg_fallback, Vec::new());
            return;
        }
        poll_local(rx, (pkg_fallback, Vec::new()), move |(detailed, alts)| {
            on_done(detailed, alts)
        });
    }

    /// Fast cross-source lookup used for install priority (no Flathub enrich / permissions).
    pub fn resolve_install_candidates_light(&self, pkg: &Package) -> Vec<Package> {
        let (pacman, flatpak, aur) = self.backends();
        let priority = self.config().advanced.clone();

        self.runtime.block_on(async {
            let search = SearchQuery {
                text: search_text_for(pkg),
                ..Default::default()
            };

            let pacman_f = async {
                if pacman.enabled() {
                    pacman.search(&search).await.ok().map(|r| r.packages)
                } else {
                    None
                }
            };
            let flatpak_f = async {
                if flatpak.enabled() {
                    flatpak.search(&search).await.ok().map(|r| r.packages)
                } else {
                    None
                }
            };
            let aur_f = async {
                if aur.enabled() {
                    aur.search(&search).await.ok().map(|r| r.packages)
                } else {
                    None
                }
            };
            let (p, f, a) = tokio::join!(pacman_f, flatpak_f, aur_f);

            let mut candidates = Vec::new();
            // Always keep the clicked package as a candidate.
            candidates.push(pkg.clone());

            for list in [p, f, a] {
                let Some(list) = list else { continue };
                for candidate in list {
                    if (packages_match(pkg, &candidate)
                        || (pkg.id.source == PackageSource::Flatpak
                            && candidate.id.source == PackageSource::Flatpak
                            && pkg.id.id == candidate.id.id))
                        && !candidates.iter().any(|c| c.id == candidate.id)
                    {
                        candidates.push(candidate);
                    }
                }
            }

            candidates.sort_by(|a, b| {
                priority
                    .priority_rank(a.id.source)
                    .cmp(&priority.priority_rank(b.id.source))
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
            candidates
        })
    }

    pub fn install_candidates_async<F>(&self, pkg: Package, on_done: F)
    where
        F: FnOnce(Vec<Package>) + 'static,
    {
        let store = self.clone();
        let (tx, rx) = mpsc::channel();
        let pkg_fallback = pkg.clone();
        if !spawn_named("tcms-install-resolve", move || {
            let _ = tx.send(store.resolve_install_candidates_light(&pkg));
        }) {
            on_done(vec![pkg_fallback]);
            return;
        }
        poll_local(rx, vec![pkg_fallback], on_done);
    }

    pub fn installed(&self) -> PackageListing {
        let mut listing = self.collect_installed();
        listing.packages = self.filter_catalog(listing.packages);
        listing
    }

    pub fn updates(&self) -> PackageListing {
        // Updates should list everything pacman/Flatpak/AUR report — do not hide
        // system/codec/driver packages behind Explore visibility toggles.
        self.collect_updates()
    }

    /// Featured home sections — apps only (never codecs/drivers/system).
    pub fn featured(&self) -> Vec<FeaturedSection> {
        let config = self.config();
        let mut sections = self.runtime.block_on(async {
            let mut sections = Vec::new();
            if config.enable_flatpak {
                let trending = fetch_flathub_collection("trending", 6);
                let popular = fetch_flathub_collection("popular", 6);
                let updated = fetch_flathub_collection("recently-updated", 6);
                let (trending, popular, updated) = tokio::join!(trending, popular, updated);

                let push = |sections: &mut Vec<FeaturedSection>,
                            id: &str,
                            title_key: &str,
                            result: tcms_core::Result<Vec<Package>>| {
                    match result {
                        Ok(mut packages) => {
                            packages.retain(|p| p.kind() == PackageKind::App);
                            if !packages.is_empty() {
                                sections.push(FeaturedSection {
                                    id: id.into(),
                                    title_key: title_key.into(),
                                    packages,
                                });
                            }
                        }
                        Err(err) => {
                            tracing::warn!(section = id, error = %err, "featured section failed")
                        }
                    }
                };

                push(&mut sections, "trending", "featured.trending", trending);
                push(&mut sections, "popular", "featured.popular", popular);
                push(&mut sections, "updated", "featured.updated", updated);
            }
            sections
        });

        // When Flatpak is off or Flathub is unreachable, spotlight local installed apps.
        if sections.is_empty() && config.enable_pacman {
            let mut installed = self.installed().packages;
            installed.retain(|p| p.kind() == PackageKind::App);
            installed.truncate(12);
            if !installed.is_empty() {
                sections.push(FeaturedSection {
                    id: "installed-spotlight".into(),
                    title_key: "featured.installed_spotlight".into(),
                    packages: installed,
                });
            }
        }

        let (_, flatpak, _) = self.backends();
        for section in &mut sections {
            for pkg in &mut section.packages {
                if pkg.id.source == PackageSource::Flatpak && pkg.id.flatpak.is_none() {
                    if let Ok(id) = self.runtime.block_on(flatpak.catalog_id(&pkg.id.id)) {
                        pkg.id = id;
                    }
                }
            }
            self.annotate_install_states(&mut section.packages);
        }
        sections
    }

    /// Mark packages that are already installed, including cross-source matches
    /// (e.g. pacman Firefox makes Flathub Firefox show as installed in lists).
    fn annotate_install_states(&self, packages: &mut [Package]) -> Vec<String> {
        if packages.is_empty() {
            return Vec::new();
        }
        let listing = self.collect_installed();
        let installed = listing.packages;
        let exact: std::collections::HashMap<_, _> = installed.iter().map(|p| (&p.id, p)).collect();
        for pkg in packages.iter_mut() {
            if let Some(exact) = exact.get(&pkg.id) {
                pkg.desktop_id = exact.desktop_id.clone();
                if pkg.state != InstallState::Updatable {
                    pkg.state = exact.state;
                }
                pkg.installed_elsewhere = false;
                if pkg.available_version.is_none() {
                    pkg.available_version = exact.available_version.clone();
                }
                if pkg.version.is_empty() || pkg.version == "installed" {
                    pkg.version = exact.version.clone();
                }
                continue;
            }

            if pkg.state != InstallState::Available {
                pkg.installed_elsewhere = false;
                continue;
            }

            if installed
                .iter()
                .any(|candidate| packages_match(pkg, candidate))
            {
                pkg.state = InstallState::Installed;
                pkg.installed_elsewhere = true;
            }
        }
        listing.errors
    }

    pub fn refresh_sources(&self) -> Vec<String> {
        let _transaction = PACKAGE_TRANSACTIONS
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (pacman, flatpak, aur) = self.backends();
        let errors = self.runtime.block_on(async {
            let mut errors = Vec::new();
            if pacman.enabled() {
                if let Err(e) = pacman.refresh().await {
                    errors.push(format!("pacman: {e}"));
                }
            }
            if flatpak.enabled() {
                if let Err(e) = flatpak.refresh().await {
                    errors.push(format!("flatpak: {e}"));
                }
            }
            if aur.enabled() {
                if let Err(e) = aur.refresh().await {
                    errors.push(format!("aur: {e}"));
                }
            }
            errors
        });
        self.installed_cache.invalidate();
        errors
    }

    pub fn refresh_async<F>(&self, on_done: F)
    where
        F: FnOnce(Vec<String>) + 'static,
    {
        let store = self.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(store.refresh_sources());
        });
        poll_local(
            rx,
            vec!["refresh worker stopped unexpectedly".into()],
            on_done,
        );
    }

    pub fn apply_action(
        &self,
        action: PackageAction,
        id: &PackageId,
        progress: mpsc::SyncSender<String>,
    ) -> tcms_core::Result<()> {
        let _ = progress.try_send(tcms_core::i18n::t("transaction.queued"));
        let _transaction = PACKAGE_TRANSACTIONS
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (pacman, flatpak, aur) = self.backends();
        let result = self
            .runtime
            .block_on(tcms_core::process::with_progress(progress, async {
                tcms_core::process::report_progress(&format!(
                    "\n=== {:?}: {} ===\n",
                    action,
                    id.display_ref()
                ));
                match id.source {
                    PackageSource::Pacman => {
                        if !pacman.enabled() {
                            return Err(tcms_core::Error::BackendDisabled("pacman".into()));
                        }
                        pacman.apply(action, id).await
                    }
                    PackageSource::Flatpak => {
                        if !flatpak.enabled() {
                            return Err(tcms_core::Error::BackendDisabled("flatpak".into()));
                        }
                        flatpak.apply(action, id).await
                    }
                    PackageSource::Aur => {
                        if !aur.enabled() {
                            return Err(tcms_core::Error::BackendDisabled("aur".into()));
                        }
                        aur.apply(action, id).await
                    }
                }
            }));
        self.installed_cache.invalidate();
        result
    }

    pub fn apply_action_async<F>(
        &self,
        action: PackageAction,
        id: PackageId,
        on_progress: impl Fn(String) + 'static,
        on_done: F,
    ) where
        F: FnOnce(tcms_core::Result<()>) + 'static,
    {
        let store = self.clone();
        let (tx, rx) = mpsc::channel();
        let (progress, progress_rx) = mpsc::sync_channel(64);
        poll_progress(progress_rx, on_progress);
        if !spawn_named("tcms-pkg-op", move || {
            let _ = tx.send(store.apply_action(action, &id, progress));
        }) {
            on_done(Err(tcms_core::Error::Message(
                "failed to start package operation thread".into(),
            )));
            return;
        }
        poll_local(
            rx,
            Err(tcms_core::Error::Message(
                "package worker stopped unexpectedly".into(),
            )),
            on_done,
        );
    }

    pub fn update_all_async<F>(&self, on_progress: impl Fn(String) + 'static, on_done: F)
    where
        F: FnOnce(UpdateReport) + 'static,
    {
        let store = self.clone();
        let (tx, rx) = mpsc::channel();
        let (progress, progress_rx) = mpsc::sync_channel(64);
        poll_progress(progress_rx, on_progress);
        if !spawn_named("tcms-update-all", move || {
            let _ = progress.try_send(tcms_core::i18n::t("transaction.queued"));
            let _transaction = PACKAGE_TRANSACTIONS
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let (pacman, flatpak, aur) = store.backends();
            let report = store.runtime.block_on(tcms_core::process::with_progress(
                progress,
                update_backends(&[&pacman, &flatpak, &aur]),
            ));
            store.installed_cache.invalidate();
            let _ = tx.send(report);
        }) {
            on_done(UpdateReport {
                completed: vec![],
                errors: vec!["failed to start update worker".into()],
            });
            return;
        }
        poll_local(
            rx,
            UpdateReport {
                completed: vec![],
                errors: vec!["update worker stopped unexpectedly".into()],
            },
            on_done,
        );
    }

    pub fn fetch_updates_async<F>(&self, on_done: F)
    where
        F: FnOnce(PackageListing) + 'static,
    {
        let store = self.clone();
        let (tx, rx) = mpsc::channel();
        if !spawn_named("tcms-check-updates", move || {
            let _ = tx.send(store.updates());
        }) {
            on_done(PackageListing {
                packages: vec![],
                errors: vec!["failed to start update check".into()],
            });
            return;
        }
        poll_local(
            rx,
            PackageListing {
                packages: vec![],
                errors: vec!["update check stopped unexpectedly".into()],
            },
            on_done,
        );
    }

    pub fn fetch_async<F>(&self, kind: ListKind, query: String, on_done: F)
    where
        F: FnOnce(PackageListing) + 'static,
    {
        let store = self.clone();
        let (tx, rx) = mpsc::channel();
        if !spawn_named("tcms-fetch", move || {
            let packages = match kind {
                ListKind::Explore => store.explore(&query),
                ListKind::Installed => store.installed(),
            };
            let _ = tx.send(packages);
        }) {
            on_done(PackageListing {
                packages: vec![],
                errors: vec!["could not start catalog worker".into()],
            });
            return;
        }
        poll_local(
            rx,
            PackageListing {
                packages: vec![],
                errors: vec!["catalog worker stopped unexpectedly".into()],
            },
            on_done,
        );
    }

    pub fn fetch_featured_async<F>(&self, on_done: F)
    where
        F: FnOnce(Vec<FeaturedSection>) + 'static,
    {
        let store = self.clone();
        let (tx, rx) = mpsc::channel();
        if !spawn_named("tcms-featured", move || {
            let _ = tx.send(store.featured());
        }) {
            on_done(Vec::new());
            return;
        }
        poll_local(rx, Vec::new(), on_done);
    }

    fn search(&self, query: SearchQuery) -> PackageListing {
        let (pacman, flatpak, aur) = self.backends();
        self.runtime.block_on(async {
            let (p, f, a) = tokio::join!(
                async {
                    if pacman.enabled() {
                        Some(pacman.search(&query).await.map(|r| r.packages))
                    } else {
                        None
                    }
                },
                async {
                    if flatpak.enabled() {
                        Some(flatpak.search(&query).await.map(|r| r.packages))
                    } else {
                        None
                    }
                },
                async {
                    if aur.enabled() {
                        Some(aur.search(&query).await.map(|r| r.packages))
                    } else {
                        None
                    }
                },
            );
            collect_results([("pacman", p), ("flatpak", f), ("aur", a)])
        })
    }

    fn collect_installed(&self) -> PackageListing {
        self.installed_cache.get_or_load(
            || self.load_installed(),
            |listing| listing.errors.is_empty(),
        )
    }

    fn load_installed(&self) -> PackageListing {
        let (pacman, flatpak, aur) = self.backends();
        self.runtime.block_on(async {
            let (mut p, f, a) = tokio::join!(
                async {
                    if pacman.enabled() {
                        Some(pacman.installed().await)
                    } else {
                        None
                    }
                },
                async {
                    if flatpak.enabled() {
                        Some(flatpak.installed().await)
                    } else {
                        None
                    }
                },
                async {
                    if aur.enabled() {
                        Some(aur.installed().await)
                    } else {
                        None
                    }
                },
            );
            if let Some(Ok(aur_packages)) = &a {
                let names: std::collections::HashSet<_> =
                    aur_packages.iter().map(|p| p.id.id.as_str()).collect();
                if let Some(Ok(desktop_packages)) = &mut p {
                    for pkg in desktop_packages {
                        if names.contains(pkg.id.id.as_str()) {
                            pkg.id.source = PackageSource::Aur;
                        }
                    }
                }
            }
            let mut listing = collect_results([("pacman", p), ("flatpak", f), ("aur", a)]);
            let mut seen = std::collections::HashSet::new();
            listing.packages.retain(|p| seen.insert(p.id.clone()));
            listing.packages.sort_by_key(|p| p.name.to_lowercase());
            listing
        })
    }

    fn collect_updates(&self) -> PackageListing {
        let (pacman, flatpak, aur) = self.backends();
        self.runtime.block_on(async {
            let mut listing = PackageListing::default();
            for backend in [&pacman as &dyn Backend, &flatpak, &aur] {
                if !backend.enabled() {
                    continue;
                }
                match backend.updates().await {
                    Ok(packages) => listing.packages.extend(packages),
                    Err(error) => listing
                        .errors
                        .push(format!("{}: {error}", backend.id().as_str())),
                }
            }
            listing
                .packages
                .sort_by_key(|package| package.name.to_lowercase());
            listing
        })
    }
}

fn collect_results<const N: usize>(
    results: [(&str, Option<tcms_core::Result<Vec<Package>>>); N],
) -> PackageListing {
    let mut listing = PackageListing::default();
    for (source, result) in results {
        match result {
            Some(Ok(packages)) => listing.packages.extend(packages),
            Some(Err(error)) => listing.errors.push(format!("{source}: {error}")),
            None => {}
        }
    }
    listing
}

impl Default for StoreService {
    fn default() -> Self {
        Self::new()
    }
}

fn poll_progress(rx: mpsc::Receiver<String>, on_progress: impl Fn(String) + 'static) {
    glib::timeout_add_local(Duration::from_millis(100), move || {
        // A bounded amount of work per GTK tick, even for verbose builds.
        for _ in 0..16 {
            match rx.try_recv() {
                Ok(text) => on_progress(text),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return glib::ControlFlow::Break,
            }
        }
        glib::ControlFlow::Continue
    });
}

fn poll_local<T, F>(rx: mpsc::Receiver<T>, fallback: T, on_done: F)
where
    T: 'static,
    F: FnOnce(T) + 'static,
{
    let mut on_done = Some(on_done);
    let mut fallback = Some(fallback);
    glib::timeout_add_local(Duration::from_millis(50), move || match rx.try_recv() {
        Ok(value) => {
            if let Some(cb) = on_done.take() {
                cb(value);
            }
            glib::ControlFlow::Break
        }
        Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(mpsc::TryRecvError::Disconnected) => {
            if let (Some(cb), Some(value)) = (on_done.take(), fallback.take()) {
                cb(value);
            }
            glib::ControlFlow::Break
        }
    });
}

fn spawn_named<F>(name: &str, f: F) -> bool
where
    F: FnOnce() + Send + 'static,
{
    match std::thread::Builder::new().name(name.into()).spawn(f) {
        Ok(_) => true,
        Err(err) => {
            tracing::error!(thread = name, error = %err, "failed to spawn worker thread");
            false
        }
    }
}

#[derive(Clone)]
pub struct UiBridge {
    pub store: StoreService,
    pub toast: libadwaita::ToastOverlay,
    pub reload: Rc<dyn Fn()>,
    pub open_detail: Rc<dyn Fn(Package)>,
    pub window: gtk4::Window,
    pub icons: crate::icon_loader::IconLoader,
    pub busy: Rc<std::cell::RefCell<std::collections::HashSet<PackageId>>>,
    /// Shown while a package transaction is running.
    pub activity: libadwaita::Banner,
    pub transaction_log: gtk4::TextBuffer,
    pub pending_transactions: Rc<std::cell::Cell<usize>>,
}

impl UiBridge {
    pub fn toast_msg(&self, message: &str) {
        self.toast.add_toast(libadwaita::Toast::new(message));
    }

    pub fn set_activity(&self, message: Option<&str>) {
        match message {
            Some(msg) => {
                self.activity.set_title(msg);
                self.activity.set_revealed(true);
            }
            None if self.pending_transactions.get() > 0 => {
                self.activity.set_title(&tcms_core::i18n::t_args(
                    "transaction.pending",
                    &[("n", &self.pending_transactions.get().to_string())],
                ));
                self.activity.set_revealed(true);
            }
            None => self.activity.set_revealed(false),
        }
    }

    pub fn append_progress(&self, text: String) {
        let buffer = &self.transaction_log;
        buffer.insert(&mut buffer.end_iter(), &text);
        let excess = buffer.char_count() - 64_000;
        if excess > 0 {
            buffer.delete(&mut buffer.start_iter(), &mut buffer.iter_at_offset(excess));
        }
    }

    pub fn package_started(&self, message: &str) {
        self.pending_transactions
            .set(self.pending_transactions.get() + 1);
        if self.pending_transactions.get() == 1 {
            self.set_activity(Some(message));
        } else {
            self.set_activity(None);
        }
    }

    pub fn package_finished(&self) {
        self.pending_transactions
            .set(self.pending_transactions.get().saturating_sub(1));
        self.set_activity(None);
    }

    pub fn launch_installed(&self, pkg: &Package) {
        let wanted = pkg.clone();
        let bridge = self.clone();
        self.store
            .fetch_async(ListKind::Installed, String::new(), move |listing| {
                let target = listing
                    .packages
                    .iter()
                    .find(|p| p.id == wanted.id)
                    .or_else(|| listing.packages.iter().find(|p| packages_match(&wanted, p)));
                let exit_bridge = bridge.clone();
                match target.and_then(|p| {
                    launch_package(p, move |result| {
                        if let Err(error) = result {
                            exit_bridge.toast_msg(&format!(
                                "{}: {error}",
                                tcms_core::i18n::t("launch.failed")
                            ));
                        }
                    })
                    .err()
                }) {
                    Some(error) => bridge
                        .toast_msg(&format!("{}: {error}", tcms_core::i18n::t("launch.failed"))),
                    None if target.is_none() => {
                        bridge.toast_msg(&tcms_core::i18n::t("launch.failed"))
                    }
                    None => {}
                }
            });
    }

    pub fn open_package(&self, pkg: &Package) {
        (self.open_detail)(pkg.clone());
    }

    pub fn run_action(&self, action: PackageAction, pkg: &Package, button: &gtk4::Button) {
        use gtk4::prelude::WidgetExt;
        button.set_sensitive(false);
        let button = button.clone();
        let done: Rc<dyn Fn()> = Rc::new(move || button.set_sensitive(true));
        if action == PackageAction::Remove {
            self.confirm_remove(pkg, done);
            return;
        }

        if action == PackageAction::Install {
            let ask = self.store.config().advanced.ask_repo_on_install;
            let bridge = self.clone();
            let store = self.store.clone();
            let pkg = pkg.clone();
            if ask {
                bridge.toast_msg(&tcms_core::i18n::t("install.resolving"));
                store.install_candidates_async(pkg, move |candidates| {
                    bridge.prompt_install_source_with(candidates, done.clone());
                });
                return;
            }
            // Install exactly the package the user clicked (source preserved).
            // Source priority only reorders the chooser when "ask repo" is enabled,
            // and ranks search results — it must not silently redirect installs.
            bridge.confirm_system_action(PackageAction::Install, &pkg, done);
            return;
        }

        self.confirm_system_action(action, pkg, done);
    }

    fn confirm_remove(&self, pkg: &Package, done: Rc<dyn Fn()>) {
        use libadwaita::prelude::*;
        use tcms_core::i18n::{t, t_args};

        let dialog = libadwaita::AlertDialog::builder()
            .heading(t("confirm.remove_title"))
            .body(t_args("confirm.remove_body", &[("name", &pkg.name)]))
            .build();
        dialog.add_response("cancel", &t("action.cancel"));
        dialog.add_response("remove", &t("action.remove"));
        dialog.set_response_appearance("remove", libadwaita::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");

        let bridge = self.clone();
        let pkg = pkg.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "remove" {
                bridge.execute_action(PackageAction::Remove, &pkg, done.clone());
            } else {
                done();
            }
        });
        dialog.present(Some(&self.window));
    }

    fn confirm_system_action(&self, action: PackageAction, pkg: &Package, done: Rc<dyn Fn()>) {
        use libadwaita::prelude::*;
        use tcms_core::i18n::t;
        if pkg.id.source != PackageSource::Pacman {
            self.execute_action(action, pkg, done);
            return;
        }
        let dialog = libadwaita::AlertDialog::builder()
            .heading(t("confirm.system_upgrade_title"))
            .body(t("confirm.system_upgrade_body"))
            .build();
        dialog.add_response("cancel", &t("action.cancel"));
        dialog.add_response("continue", &t("action.update"));
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let bridge = self.clone();
        let pkg = pkg.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "continue" {
                bridge.execute_action(action, &pkg, done.clone());
            } else {
                done();
            }
        });
        dialog.present(Some(&self.window));
    }

    fn execute_action(&self, action: PackageAction, pkg: &Package, done: Rc<dyn Fn()>) {
        use tcms_core::i18n::t_args;

        if !self.busy.borrow_mut().insert(pkg.id.clone()) {
            self.toast_msg(&t_args("toast.busy", &[("name", &pkg.name)]));
            done();
            return;
        }

        let name = pkg.name.clone();
        let start_key = match action {
            PackageAction::Install => "toast.installing",
            PackageAction::Remove => "toast.removing",
            PackageAction::Update => "toast.updating",
        };
        let activity_msg = t_args(start_key, &[("name", &name)]);
        self.package_started(&activity_msg);
        self.toast_msg(&activity_msg);

        let bridge = self.clone();
        let pkg_id = pkg.id.clone();
        let pkg_for_open = pkg.clone();
        self.store.apply_action_async(
            action,
            pkg.id.clone(),
            {
                let bridge = self.clone();
                move |text| bridge.append_progress(text)
            },
            move |result| {
                bridge.busy.borrow_mut().remove(&pkg_id);
                bridge.package_finished();
                done();
                match result {
                    Ok(()) => {
                        if action == PackageAction::Install {
                            bridge.toast_installed_with_open(&pkg_for_open);
                        } else {
                            bridge.toast_msg(&t_args("toast.done", &[("name", &name)]));
                        }
                        (bridge.reload)();
                    }
                    Err(err) => {
                        bridge.toast_msg(&t_args(
                            "toast.failed",
                            &[("name", &name), ("error", &err.to_string())],
                        ));
                    }
                }
            },
        );
    }

    fn toast_installed_with_open(&self, pkg: &Package) {
        use tcms_core::i18n::t_args;
        let toast = libadwaita::Toast::new(&t_args("toast.done", &[("name", &pkg.name)]));
        toast.set_button_label(Some(&tcms_core::i18n::t("action.open")));
        let pkg = pkg.clone();
        let bridge = self.clone();
        toast.connect_button_clicked(move |_| {
            bridge.launch_installed(&pkg);
        });
        self.toast.add_toast(toast);
    }

    fn prompt_install_source_with(&self, candidates: Vec<Package>, done: Rc<dyn Fn()>) {
        use libadwaita::prelude::*;
        use tcms_core::i18n::t;

        if candidates.is_empty() {
            done();
            return;
        }
        if candidates.len() == 1 {
            self.confirm_system_action(PackageAction::Install, &candidates[0], done);
            return;
        }

        let name = candidates[0].name.clone();
        let dialog = libadwaita::AlertDialog::builder()
            .heading(t("install.choose_source"))
            .body(t_args_simple("install.choose_source_body", &name))
            .build();
        for (idx, candidate) in candidates.iter().enumerate() {
            let label = format!(
                "{} — {}",
                t(candidate.id.source.i18n_key()),
                candidate.id.display_ref()
            );
            dialog.add_response(&idx.to_string(), &label);
        }
        dialog.add_response("cancel", &t("action.cancel"));
        dialog.set_default_response(Some("0"));
        dialog.set_close_response("cancel");

        let bridge = self.clone();
        let candidates = candidates.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "cancel" {
                done();
                return;
            }
            if let Ok(idx) = response.parse::<usize>() {
                if let Some(pkg) = candidates.get(idx) {
                    bridge.confirm_system_action(PackageAction::Install, pkg, done.clone());
                    return;
                }
            }
            done();
        });
        dialog.present(Some(&self.window));
    }
}

fn t_args_simple(key: &str, name: &str) -> String {
    tcms_core::i18n::t_args(key, &[("name", name)])
}

/// Launch an actual desktop entry; never treat spawning gtk-launch as success.
fn launch_package(
    pkg: &Package,
    on_exit: impl FnOnce(Result<(), glib::Error>) + 'static,
) -> Result<(), String> {
    match pkg.id.source {
        PackageSource::Flatpak => {
            let reference = pkg.id.flatpak_ref().map_err(|e| e.to_string())?;
            let scope = &pkg.id.flatpak.as_ref().unwrap().installation;
            let args = ["flatpak", "run", scope.flag(), reference.as_str()];
            let args: Vec<_> = args.iter().map(std::ffi::OsStr::new).collect();
            let process = gio::Subprocess::newv(&args, gio::SubprocessFlags::NONE)
                .map_err(|e| e.to_string())?;
            process.wait_check_async(gio::Cancellable::NONE, on_exit);
            Ok(())
        }
        PackageSource::Pacman | PackageSource::Aur => {
            let id = pkg
                .desktop_id
                .as_deref()
                .ok_or_else(|| "desktop entry unavailable".to_string())?;
            let info = gio::DesktopAppInfo::new(id)
                .ok_or_else(|| format!("desktop entry not found: {id}"))?;
            info.launch(&[], gio::AppLaunchContext::NONE)
                .map_err(|e| e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn partial_catalog_failure_retains_successful_sources() {
        let pkg = Package::stub(
            PackageSource::Pacman,
            "example",
            "Example",
            "",
            "1",
            InstallState::Installed,
        );
        let result = collect_results([
            ("pacman", Some(Ok(vec![pkg]))),
            (
                "flatpak",
                Some(Err(tcms_core::Error::Message("offline".into()))),
            ),
            ("aur", None),
        ]);
        assert_eq!(result.packages.len(), 1);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("flatpak"));
    }

    #[test]
    fn worker_disconnect_completes_callback_instead_of_leaving_ui_busy() {
        let context = glib::MainContext::default();
        let _guard = context.acquire().unwrap();
        let (tx, rx) = mpsc::channel::<Result<(), String>>();
        drop(tx);
        let completed = Rc::new(RefCell::new(None));
        let result = completed.clone();
        poll_local(rx, Err("worker stopped".into()), move |value| {
            *result.borrow_mut() = Some(value);
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while completed.borrow().is_none() && std::time::Instant::now() < deadline {
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(*completed.borrow(), Some(Err("worker stopped".into())));
    }
}
