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
