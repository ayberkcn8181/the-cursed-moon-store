use std::rc::Rc;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use gtk4::prelude::IsA;
use tcms_aur::AurBackend;
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
    Updates,
}

#[derive(Clone)]
pub struct StoreService {
    inner: Arc<std::sync::Mutex<StoreInner>>,
    runtime: Arc<tokio::runtime::Runtime>,
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
            Ok(())
        })
    }

    fn filter_catalog(&self, mut packages: Vec<Package>) -> Vec<Package> {
        let config = self.config();
        packages.retain(|p| config.allows_package(p));
        packages
    }

    pub fn explore(&self, text: &str) -> Vec<Package> {
        if text.trim().is_empty() {
            return Vec::new();
        }
        let mut packages = self.filter_catalog(self.search(SearchQuery {
            text: text.to_string(),
            ..Default::default()
        }));
        self.annotate_install_states(&mut packages);
        let priority = self.config().advanced.clone();
        packages.sort_by(|a, b| {
            priority
                .priority_rank(a.id.source)
                .cmp(&priority.priority_rank(b.id.source))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        packages
    }

    /// Rich package details for the detail page, plus alternate sources.
    pub fn package_details(&self, pkg: &Package) -> (Package, Vec<Package>) {
        let detailed = self.enrich_one(pkg);
        let mut alts = self.resolve_install_candidates_light(pkg);
        alts.retain(|p| p.id != detailed.id);
        (detailed, alts)
    }

    fn enrich_one(&self, pkg: &Package) -> Package {
        let (pacman, flatpak, aur) = self.backends();
        let id = pkg.id.clone();
        self.runtime
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
            .unwrap_or_else(|| pkg.clone())
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
        poll_local(rx, move |(detailed, alts)| on_done(detailed, alts));
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
                    if packages_match(pkg, &candidate)
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
        poll_local(rx, on_done);
    }

    pub fn installed(&self) -> Vec<Package> {
        self.filter_catalog(self.collect_installed())
    }

    pub fn updates(&self) -> Vec<Package> {
        // Respect General → visibility toggles so system/codec floods stay hidden by default.
        self.filter_catalog(self.collect_updates())
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
            let mut installed = self.installed();
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

        for section in &mut sections {
            self.annotate_install_states(&mut section.packages);
        }
        sections
    }

    /// Mark packages that are already installed (exact id match per source).
    fn annotate_install_states(&self, packages: &mut [Package]) {
        if packages.is_empty() {
            return;
        }
        let installed = self.collect_installed();
        let mut by_id = std::collections::HashMap::new();
        for pkg in &installed {
            by_id.insert(pkg.id.clone(), pkg.state);
        }
        for pkg in packages.iter_mut() {
            if let Some(state) = by_id.get(&pkg.id) {
                if pkg.state == InstallState::Available {
                    pkg.state = *state;
                }
            }
        }
    }

    pub fn refresh_sources(&self) -> Vec<String> {
        let (pacman, flatpak, aur) = self.backends();
        self.runtime.block_on(async {
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
        })
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
        poll_local(rx, on_done);
    }

    pub fn apply_action(&self, action: PackageAction, id: &PackageId) -> tcms_core::Result<()> {
        let (pacman, flatpak, aur) = self.backends();
        self.runtime.block_on(async {
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
        })
    }

    pub fn apply_action_async<F>(&self, action: PackageAction, id: PackageId, on_done: F)
    where
        F: FnOnce(tcms_core::Result<()>) + 'static,
    {
        let store = self.clone();
        let (tx, rx) = mpsc::channel();
        if !spawn_named("tcms-pkg-op", move || {
            let _ = tx.send(store.apply_action(action, &id));
        }) {
            on_done(Err(tcms_core::Error::Message(
                "failed to start package operation thread".into(),
            )));
            return;
        }
        poll_local(rx, on_done);
    }

    pub fn update_all_async<F>(&self, on_done: F)
    where
        F: FnOnce(tcms_core::Result<usize>) + 'static,
    {
        let store = self.clone();
        let (tx, rx) = mpsc::channel();
        if !spawn_named("tcms-update-all", move || {
            let updates = store.updates();
            let mut ok = 0usize;
            let mut last_err = None;
            for pkg in updates {
                match store.apply_action(PackageAction::Update, &pkg.id) {
                    Ok(()) => ok += 1,
                    Err(e) => last_err = Some(e),
                }
            }
            let result = match last_err {
                Some(e) if ok == 0 => Err(e),
                _ => Ok(ok),
            };
            let _ = tx.send(result);
        }) {
            on_done(Err(tcms_core::Error::Message(
                "failed to start update-all thread".into(),
            )));
            return;
        }
        poll_local(rx, on_done);
    }

    pub fn fetch_async<F>(&self, kind: ListKind, query: String, on_done: F)
    where
        F: FnOnce(Vec<Package>) + 'static,
    {
        let store = self.clone();
        let (tx, rx) = mpsc::channel();
        if !spawn_named("tcms-fetch", move || {
            let packages = match kind {
                ListKind::Explore => store.explore(&query),
                ListKind::Installed => store.installed(),
                ListKind::Updates => store.updates(),
            };
            let _ = tx.send(packages);
        }) {
            on_done(Vec::new());
            return;
        }
        poll_local(rx, on_done);
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
        poll_local(rx, on_done);
    }

    fn search(&self, query: SearchQuery) -> Vec<Package> {
        let (pacman, flatpak, aur) = self.backends();
        self.runtime.block_on(async {
            let pacman_f = async {
                if pacman.enabled() {
                    pacman.search(&query).await.ok().map(|r| r.packages)
                } else {
                    None
                }
            };
            let flatpak_f = async {
                if flatpak.enabled() {
                    flatpak.search(&query).await.ok().map(|r| r.packages)
                } else {
                    None
                }
            };
            let aur_f = async {
                if aur.enabled() {
                    aur.search(&query).await.ok().map(|r| r.packages)
                } else {
                    None
                }
            };
            let (p, f, a) = tokio::join!(pacman_f, flatpak_f, aur_f);
            let mut packages = Vec::new();
            for list in [p, f, a].into_iter().flatten() {
                packages.extend(list);
            }
            packages
        })
    }

    fn collect_installed(&self) -> Vec<Package> {
        let (pacman, flatpak, aur) = self.backends();
        self.runtime.block_on(async {
            let pacman_f = async {
                if pacman.enabled() {
                    pacman.installed().await.ok()
                } else {
                    None
                }
            };
            let flatpak_f = async {
                if flatpak.enabled() {
                    flatpak.installed().await.ok()
                } else {
                    None
                }
            };
            let aur_f = async {
                if aur.enabled() {
                    aur.installed().await.ok()
                } else {
                    None
                }
            };
            let (p, f, a) = tokio::join!(pacman_f, flatpak_f, aur_f);
            let mut packages = Vec::new();
            for list in [p, f, a].into_iter().flatten() {
                packages.extend(list);
            }
            let mut seen = std::collections::HashSet::new();
            packages.sort_by(|a, b| {
                let rank = |p: &Package| match p.id.source {
                    PackageSource::Flatpak => 0,
                    PackageSource::Pacman => 1,
                    PackageSource::Aur => 2,
                };
                rank(a)
                    .cmp(&rank(b))
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
            // Deduplicate by full PackageId (source + id), not bare name —
            // the same app can legitimately exist in pacman and Flatpak.
            packages.retain(|p| seen.insert(p.id.clone()));
            packages.sort_by_key(|a| a.name.to_lowercase());
            packages
        })
    }

    fn collect_updates(&self) -> Vec<Package> {
        let (pacman, flatpak, aur) = self.backends();
        self.runtime.block_on(async {
            let pacman_f = async {
                if pacman.enabled() {
                    pacman.updates().await.ok()
                } else {
                    None
                }
            };
            let flatpak_f = async {
                if flatpak.enabled() {
                    flatpak.updates().await.ok()
                } else {
                    None
                }
            };
            let aur_f = async {
                if aur.enabled() {
                    aur.updates().await.ok()
                } else {
                    None
                }
            };
            let (p, f, a) = tokio::join!(pacman_f, flatpak_f, aur_f);
            let mut packages = Vec::new();
            for list in [p, f, a].into_iter().flatten() {
                packages.extend(list);
            }
            packages.sort_by_key(|a| a.name.to_lowercase());
            packages
        })
    }
}

impl Default for StoreService {
    fn default() -> Self {
        Self::new()
    }
}

fn poll_local<T, F>(rx: mpsc::Receiver<T>, on_done: F)
where
    T: 'static,
    F: FnOnce(T) + 'static,
{
    let mut on_done = Some(on_done);
    glib::timeout_add_local(Duration::from_millis(50), move || match rx.try_recv() {
        Ok(value) => {
            if let Some(cb) = on_done.take() {
                cb(value);
            }
            glib::ControlFlow::Break
        }
        Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
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
            None => self.activity.set_revealed(false),
        }
    }

    pub fn open_package(&self, pkg: &Package) {
        (self.open_detail)(pkg.clone());
    }

    pub fn run_action(&self, action: PackageAction, pkg: &Package) {
        if action == PackageAction::Remove {
            self.confirm_remove(pkg);
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
                    bridge.prompt_install_source_with(candidates);
                });
                return;
            }
            // Install exactly the package the user clicked (source preserved).
            // Source priority only reorders the chooser when "ask repo" is enabled,
            // and ranks search results — it must not silently redirect installs.
            bridge.execute_action(PackageAction::Install, &pkg);
            return;
        }

        self.execute_action(action, pkg);
    }

    fn confirm_remove(&self, pkg: &Package) {
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
                bridge.execute_action(PackageAction::Remove, &pkg);
            }
        });
        dialog.present(Some(&self.window));
    }

    fn execute_action(&self, action: PackageAction, pkg: &Package) {
        use tcms_core::i18n::t_args;

        if !self.busy.borrow_mut().insert(pkg.id.clone()) {
            self.toast_msg(&t_args("toast.busy", &[("name", &pkg.name)]));
            return;
        }

        let name = pkg.name.clone();
        let start_key = match action {
            PackageAction::Install => "toast.installing",
            PackageAction::Remove => "toast.removing",
            PackageAction::Update => "toast.updating",
        };
        let activity_msg = t_args(start_key, &[("name", &name)]);
        self.set_activity(Some(&activity_msg));
        self.toast_msg(&activity_msg);

        let bridge = self.clone();
        let pkg_id = pkg.id.clone();
        let pkg_for_open = pkg.clone();
        self.store
            .apply_action_async(action, pkg.id.clone(), move |result| {
                bridge.busy.borrow_mut().remove(&pkg_id);
                bridge.set_activity(None);
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
            });
    }

    fn toast_installed_with_open(&self, pkg: &Package) {
        use tcms_core::i18n::t_args;
        let toast = libadwaita::Toast::new(&t_args("toast.done", &[("name", &pkg.name)]));
        toast.set_button_label(Some(&tcms_core::i18n::t("action.open")));
        let pkg = pkg.clone();
        let window = self.window.clone();
        toast.connect_button_clicked(move |_| {
            let _ = launch_package(&pkg, &window);
        });
        self.toast.add_toast(toast);
    }

    fn prompt_install_source_with(&self, candidates: Vec<Package>) {
        use libadwaita::prelude::*;
        use tcms_core::i18n::t;

        if candidates.is_empty() {
            return;
        }
        if candidates.len() == 1 {
            self.execute_action(PackageAction::Install, &candidates[0]);
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
                candidate.id.id
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
                return;
            }
            if let Ok(idx) = response.parse::<usize>() {
                if let Some(pkg) = candidates.get(idx) {
                    bridge.execute_action(PackageAction::Install, pkg);
                }
            }
        });
        dialog.present(Some(&self.window));
    }
}

fn t_args_simple(key: &str, name: &str) -> String {
    tcms_core::i18n::t_args(key, &[("name", name)])
}

/// Best-effort launch of an installed app (Flatpak or desktop file).
pub fn launch_package(pkg: &Package, parent: &impl IsA<gtk4::Window>) -> bool {
    match pkg.id.source {
        PackageSource::Flatpak => std::process::Command::new("flatpak")
            .args(["run", &pkg.id.id])
            .spawn()
            .is_ok(),
        PackageSource::Pacman | PackageSource::Aur => {
            let ids = [
                pkg.id.id.clone(),
                pkg.name.to_ascii_lowercase().replace(' ', "-"),
            ];
            for id in ids {
                if std::process::Command::new("gtk-launch")
                    .arg(&id)
                    .spawn()
                    .is_ok()
                {
                    return true;
                }
            }
            if let Some(home) = pkg.homepage.as_deref() {
                let launcher = gtk4::UriLauncher::new(home);
                launcher.launch(Some(parent), gio::Cancellable::NONE, |_| {});
                return true;
            }
            false
        }
    }
}
