//! Core types shared by The Cursed Moon Store backends and UI.

pub mod backend;
pub mod classify;
pub mod config;
pub mod error;
pub mod featured;
pub mod i18n;
pub mod icons;
pub mod matching;
pub mod package;
pub mod process;
pub mod util;

pub use backend::{Backend, BackendId, PackageAction, SearchQuery, SearchResult};
pub use classify::{classify_package, PackageKind};
pub use config::{AdvancedConfig, AppConfig, CompatibilityConfig, RepoOverride};
pub use error::{Error, Result};
pub use featured::{fetch_flathub_app, fetch_flathub_collection, FeaturedSection};
pub use i18n::{t, t_args, Language};
pub use icons::{cached_icon_path_if_exists, ensure_cached_icon, icon_cache_dir, is_remote_icon};
pub use matching::{
    assert_safe_package_id, is_safe_pkg_token, normalize_app_key, packages_match, search_text_for,
};
pub use package::{license_is_proprietary, InstallState, Package, PackageId, PackageSource};
pub use process::{
    pkexec_path, resolve_program, run, run_checked, run_privileged, run_privileged_pacman,
};
pub use util::urlencoding;
