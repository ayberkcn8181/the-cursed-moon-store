use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::package::{Package, PackageId, PackageSource};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendId {
    Pacman,
    Flatpak,
    Aur,
}

impl BackendId {
    pub fn source(self) -> PackageSource {
        match self {
            Self::Pacman => PackageSource::Pacman,
            Self::Flatpak => PackageSource::Flatpak,
            Self::Aur => PackageSource::Aur,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pacman => "pacman",
            Self::Flatpak => "flatpak",
            Self::Aur => "aur",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    pub text: String,
    pub category: Option<String>,
    pub installed_only: bool,
    pub updates_only: bool,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub packages: Vec<Package>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageAction {
    Install,
    Remove,
    Update,
}

/// Common interface implemented by pacman, Flatpak, and AUR backends.
#[async_trait]
pub trait Backend: Send + Sync {
    fn id(&self) -> BackendId;
    fn enabled(&self) -> bool;
    fn set_enabled(&mut self, enabled: bool);

    async fn refresh(&self) -> Result<()>;
    async fn search(&self, query: &SearchQuery) -> Result<SearchResult>;
    async fn get_package(&self, id: &PackageId) -> Result<Option<Package>>;
    async fn installed(&self) -> Result<Vec<Package>>;
    async fn updates(&self) -> Result<Vec<Package>>;
    async fn install(&self, id: &PackageId) -> Result<()>;
    async fn remove(&self, id: &PackageId) -> Result<()>;

    async fn apply(&self, action: PackageAction, id: &PackageId) -> Result<()> {
        match action {
            PackageAction::Install | PackageAction::Update => self.install(id).await,
            PackageAction::Remove => self.remove(id).await,
        }
    }
}
