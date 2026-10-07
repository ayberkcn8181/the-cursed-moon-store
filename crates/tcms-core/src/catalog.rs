use crate::{Package, PackageSource};
pub const PAGE_SIZE: usize = 60;
pub fn page(
    packages: &[Package],
    source: Option<PackageSource>,
    requested: usize,
) -> (Vec<Package>, usize, usize) {
    let all: Vec<_> = packages
        .iter()
        .filter(|p| source.is_none_or(|s| p.id.source == s))
        .collect();
    let total = all.len();
    let page = requested.min(total.saturating_sub(1) / PAGE_SIZE);
    (
        all.into_iter()
            .skip(page * PAGE_SIZE)
            .take(PAGE_SIZE)
            .cloned()
            .collect(),
        page,
        total,
    )
}
#[cfg(test)]
mod tests {
    #[test]
    fn source_filter_and_out_of_range_pages_are_clamped() {
        use crate::{InstallState, Package, PackageSource};
        let packages = vec![
            Package::stub(
                PackageSource::Pacman,
                "a",
                "a",
                "",
                "1",
                InstallState::Available,
            ),
            Package::stub(
                PackageSource::Aur,
                "b",
                "b",
                "",
                "1",
                InstallState::Available,
            ),
        ];
        let (rows, current, total) = super::page(&packages, Some(PackageSource::Aur), 999);
        assert_eq!(rows[0].id.id, "b");
        assert_eq!((current, total), (0, 1));
        let (rows, current, total) = super::page(&packages, Some(PackageSource::Flatpak), 999);
        assert!(rows.is_empty());
        assert_eq!((current, total), (0, 0));
    }
    #[test]
    fn every_result_is_reachable() {
        use crate::{InstallState, Package, PackageSource};
        let packages: Vec<_> = (0..185)
            .map(|i| {
                Package::stub(
                    PackageSource::Pacman,
                    &i.to_string(),
                    "",
                    "",
                    "",
                    InstallState::Available,
                )
            })
            .collect();
        let mut all = Vec::new();
        for n in 0..4 {
            all.extend(super::page(&packages, None, n).0);
        }
        assert_eq!(all.len(), 185);
        assert_eq!(all.last().unwrap().id.id, "184");
    }
}
