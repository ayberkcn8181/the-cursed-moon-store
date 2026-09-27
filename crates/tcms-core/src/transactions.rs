//! Transaction coordination shared by all windows of the application.

use std::sync::Mutex;

use crate::{Backend, BackendId, Package};

/// Pacman and AUR share a database. Keep the lock for the entire operation,
/// including a batch upgrade, rather than only while starting a child process.
pub static PACKAGE_TRANSACTIONS: Mutex<()> = Mutex::new(());

#[derive(Debug, Default)]
pub struct PackageListing {
    pub packages: Vec<Package>,
    pub errors: Vec<String>,
}

#[derive(Debug, Default)]
pub struct UpdateReport {
    pub completed: Vec<BackendId>,
    pub errors: Vec<String>,
}

/// System packages precede AUR builds. A failed system upgrade must not be
/// followed by AUR builds against a potentially inconsistent system.
pub async fn update_backends(backends: &[&dyn Backend]) -> UpdateReport {
    let mut report = UpdateReport::default();
    let mut system_failed = false;
    for id in [BackendId::Pacman, BackendId::Flatpak, BackendId::Aur] {
        let Some(backend) = backends.iter().find(|b| b.id() == id && b.enabled()) else {
            continue;
        };
        if id == BackendId::Aur && system_failed {
            report
                .errors
                .push("aur: skipped because the system upgrade failed".into());
            continue;
        }
        match backend.update_all().await {
            Ok(()) => report.completed.push(id),
            Err(error) => {
                system_failed |= id == BackendId::Pacman;
                report.errors.push(format!("{}: {error}", id.as_str()));
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Error, PackageAction, PackageId, Result, SearchQuery, SearchResult};
    use std::sync::{Arc, Mutex};

    struct FakeBackend {
        id: BackendId,
        fail: bool,
        calls: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl Backend for FakeBackend {
        fn id(&self) -> BackendId {
            self.id
        }
        fn enabled(&self) -> bool {
            true
        }
        fn set_enabled(&mut self, _: bool) {}
        async fn refresh(&self) -> Result<()> {
            Ok(())
        }
        async fn search(&self, _: &SearchQuery) -> Result<SearchResult> {
            Ok(SearchResult {
                packages: vec![],
                truncated: false,
            })
        }
        async fn get_package(&self, _: &PackageId) -> Result<Option<Package>> {
            Ok(None)
        }
        async fn installed(&self) -> Result<Vec<Package>> {
            Ok(vec![])
        }
        async fn updates(&self) -> Result<Vec<Package>> {
            Ok(vec![])
        }
        async fn install(&self, _: &PackageId) -> Result<()> {
            self.calls.lock().unwrap().push("install".into());
            Ok(())
        }
        async fn remove(&self, _: &PackageId) -> Result<()> {
            Ok(())
        }
        async fn update(&self, _: &PackageId) -> Result<()> {
            self.calls.lock().unwrap().push("update".into());
            Ok(())
        }
        async fn update_all(&self) -> Result<()> {
            self.calls.lock().unwrap().push(self.id.as_str().into());
            if self.fail {
                Err(Error::Message("failed".into()))
            } else {
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn update_does_not_dispatch_to_install() {
        let calls = Arc::new(Mutex::new(vec![]));
        let backend = FakeBackend {
            id: BackendId::Flatpak,
            fail: false,
            calls: calls.clone(),
        };
        backend
            .apply(
                PackageAction::Update,
                &PackageId::new(crate::PackageSource::Flatpak, "org.example.App"),
            )
            .await
            .unwrap();
        assert_eq!(*calls.lock().unwrap(), vec!["update"]);
    }

    #[tokio::test]
    async fn failed_system_upgrade_preserves_errors_and_skips_aur() {
        let calls = Arc::new(Mutex::new(vec![]));
        let pacman = FakeBackend {
            id: BackendId::Pacman,
            fail: true,
            calls: calls.clone(),
        };
        let flatpak = FakeBackend {
            id: BackendId::Flatpak,
            fail: false,
            calls: calls.clone(),
        };
        let aur = FakeBackend {
            id: BackendId::Aur,
            fail: false,
            calls: calls.clone(),
        };
        let report = update_backends(&[&aur, &flatpak, &pacman]).await;
        assert_eq!(*calls.lock().unwrap(), vec!["pacman", "flatpak"]);
        assert_eq!(report.completed, vec![BackendId::Flatpak]);
        assert_eq!(report.errors.len(), 2);
    }

    #[tokio::test]
    async fn upgrades_each_source_once_in_dependency_order() {
        let calls = Arc::new(Mutex::new(vec![]));
        let pacman = FakeBackend {
            id: BackendId::Pacman,
            fail: false,
            calls: calls.clone(),
        };
        let flatpak = FakeBackend {
            id: BackendId::Flatpak,
            fail: false,
            calls: calls.clone(),
        };
        let aur = FakeBackend {
            id: BackendId::Aur,
            fail: false,
            calls: calls.clone(),
        };
        let report = update_backends(&[&aur, &flatpak, &pacman]).await;
        assert_eq!(*calls.lock().unwrap(), vec!["pacman", "flatpak", "aur"]);
        assert!(report.errors.is_empty());
    }
}
