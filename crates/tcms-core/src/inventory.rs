//! Installed inventory filters. Missing metadata stays explicitly unknown.
use crate::{FlatpakInstallation, ForeignStatus, Package, PackageSource};
#[derive(Default, Clone)]
pub struct Filter { pub query: String, pub kind: u32, pub source: u32, pub size: u32 }
impl Filter {
    pub fn matches(&self, p: &Package) -> bool {
        let text = self.query.is_empty() || [&p.name, &p.id.id, &p.summary].iter().any(|s| s.to_lowercase().contains(&self.query));
        let kind = match self.kind {
            1 => p.desktop_id.is_some() || p.id.flatpak.as_ref().is_some_and(|r| r.kind == "app"),
            2 => p.explicitly_installed == Some(true), 3 => p.explicitly_installed == Some(false), 4 => p.explicitly_installed.is_none(), _ => true,
        };
        let source = match self.source {
            1 => p.id.source == PackageSource::Pacman && p.foreign_status.is_none(),
            2 => p.id.source == PackageSource::Aur || p.foreign_status == Some(ForeignStatus::InAur),
            3 => p.foreign_status == Some(ForeignStatus::Local), 4 => p.foreign_status == Some(ForeignStatus::Unverified),
            5 | 6 => p.id.flatpak.as_ref().is_some_and(|r| r.installation == if self.source == 5 { FlatpakInstallation::User } else { FlatpakInstallation::System }), _ => true,
        };
        let size = match self.size { 1 => p.size_bytes.is_some_and(|n| n >= 100 * 1024 * 1024), 2 => p.size_bytes.is_some_and(|n| n >= 1024 * 1024 * 1024), 3 => p.size_bytes.is_none(), _ => true };
        text && kind && source && size
    }
}
pub fn compare(a: &Package, b: &Package, largest_first: bool) -> std::cmp::Ordering {
    let size = if largest_first { b.size_bytes.cmp(&a.size_bytes) } else { std::cmp::Ordering::Equal };
    size.then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())).then_with(|| a.id.to_string().cmp(&b.id.to_string())).then_with(|| a.id.display_ref().cmp(&b.id.display_ref()))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FlatpakRef, InstallState};
    fn package() -> Package { Package::stub(PackageSource::Pacman, "example", "Example", "Editor", "1", InstallState::Installed) }
    #[test]
    fn missing_reason_is_never_guessed_from_a_launcher() {
        let mut p = package(); p.desktop_id = Some("example.desktop".into());
        assert!(Filter { kind: 1, ..Default::default() }.matches(&p));
        for kind in [2,3] { assert!(!Filter { kind, ..Default::default() }.matches(&p)); }
        assert!(Filter { kind: 4, ..Default::default() }.matches(&p));
        p.explicitly_installed = Some(false); assert!(Filter { kind: 3, ..Default::default() }.matches(&p));
    }
    #[test]
    fn filters_intersect_and_scopes_stay_distinct() {
        let mut p = package(); p.id.source = PackageSource::Flatpak;
        p.id.flatpak = Some(FlatpakRef { kind: "app".into(), arch: "x86_64".into(), branch: "stable".into(), origin: "flathub".into(), installation: FlatpakInstallation::User });
        p.size_bytes = Some(2 * 1024 * 1024 * 1024);
        let mut filter = Filter { query: "editor".into(), kind: 1, source: 5, size: 2 }; assert!(filter.matches(&p));
        filter.source = 6; assert!(!filter.matches(&p)); filter.source = 5; filter.size = 3; assert!(!filter.matches(&p));
        p.size_bytes = None; assert!(filter.matches(&p));
    }
    #[test]
    fn source_classification_and_unknown_sizes_are_preserved() {
        let mut p = package();
        for (status, source) in [(ForeignStatus::InAur, 2), (ForeignStatus::Local, 3), (ForeignStatus::Unverified, 4)] {
            p.foreign_status = Some(status); assert!(!Filter { source: 1, ..Default::default() }.matches(&p)); assert!(Filter { source, ..Default::default() }.matches(&p));
        }
        let mut known = p.clone(); known.size_bytes = Some(0); assert!(compare(&known, &p, true).is_lt()); assert!(Filter::default().matches(&p));
    }
}
