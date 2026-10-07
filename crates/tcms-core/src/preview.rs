use crate::{PackageAction, PackageId};
#[derive(Debug, Clone)]
pub struct PreviewEntry {
    pub id: PackageId,
    pub action: PackageAction,
    pub old_version: Option<String>,
    pub new_version: Option<String>,
    pub download_bytes: Option<u64>,
    pub disk_delta: Option<i128>,
}
#[derive(Debug, Clone, Default)]
pub struct TransactionPreview {
    pub entries: Vec<PreviewEntry>,
    pub notes: Vec<String>,
}
impl TransactionPreview {
    pub fn extend(&mut self, other: Self) {
        for e in other.entries {
            if !self.entries.iter().any(|p| p.id == e.id) {
                self.entries.push(e);
            }
        }
        for n in other.notes {
            if !self.notes.contains(&n) {
                self.notes.push(n);
            }
        }
    }
    pub fn download_bytes(&self) -> Option<u64> {
        self.entries.iter().map(|e| e.download_bytes).sum()
    }
    pub fn disk_delta(&self) -> Option<i128> {
        self.entries.iter().map(|e| e.disk_delta).sum()
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn unknown_sizes_are_not_zero() {
        let p = super::TransactionPreview {
            entries: vec![super::PreviewEntry {
                id: crate::PackageId::new(crate::PackageSource::Aur, "test"),
                action: crate::PackageAction::Install,
                old_version: None,
                new_version: None,
                download_bytes: None,
                disk_delta: None,
            }],
            notes: vec![],
        };
        assert_eq!(p.download_bytes(), None);
        assert_eq!(p.disk_delta(), None);
    }
}
