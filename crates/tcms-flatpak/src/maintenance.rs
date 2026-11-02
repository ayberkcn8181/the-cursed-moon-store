use crate::FlatpakBackend;
use libflatpak::{prelude::*, Installation};
use tcms_core::{process, t, Error, FlatpakInstallation, Result};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnusedRef {
    pub reference: String,
    pub commit: String,
    pub size: u64,
}
fn installation(scope: &FlatpakInstallation) -> Result<Installation> {
    match scope {
        FlatpakInstallation::User => Installation::new_user(gio::Cancellable::NONE),
        FlatpakInstallation::System => Installation::new_system(gio::Cancellable::NONE),
    }
    .map_err(|e| Error::Message(e.to_string()))
}
fn list(scope: &FlatpakInstallation) -> Result<Vec<UnusedRef>> {
    let mut refs: Vec<_> = installation(scope)?
        .list_unused_refs(None, gio::Cancellable::NONE)
        .map_err(|e| Error::Message(e.to_string()))?
        .into_iter()
        .map(|r| {
            Ok(UnusedRef {
                reference: r
                    .format_ref()
                    .ok_or_else(|| Error::Message("Missing unused ref".into()))?
                    .to_string(),
                commit: r
                    .commit()
                    .ok_or_else(|| Error::Message("Missing unused ref commit".into()))?
                    .to_string(),
                size: r.installed_size(),
            })
        })
        .collect::<Result<_>>()?;
    refs.sort_by(|a, b| a.reference.cmp(&b.reference));
    Ok(refs)
}
pub(crate) fn installed_sizes(
    scope: FlatpakInstallation,
) -> Result<std::collections::HashMap<String, u64>> {
    Ok(installation(&scope)?
        .list_installed_refs(gio::Cancellable::NONE)
        .map_err(|e| Error::Message(e.to_string()))?
        .into_iter()
        .filter_map(|r| Some((r.format_ref()?.to_string(), r.installed_size())))
        .collect())
}
impl FlatpakBackend {
    pub async fn unused(&self, scope: FlatpakInstallation) -> Result<Vec<UnusedRef>> {
        self.ensure_enabled()?;
        tokio::task::spawn_blocking(move || list(&scope))
            .await
            .map_err(|e| Error::Message(e.to_string()))?
    }
    pub async fn remove_unused(
        &self,
        scope: FlatpakInstallation,
        expected: &[UnusedRef],
    ) -> Result<()> {
        if expected.is_empty() {
            return Err(Error::Message("No unused refs selected".into()));
        }
        if self.unused(scope.clone()).await? != expected {
            return Err(Error::Message(t("management.changed")));
        }
        // Only the refs included in the preview are explicit targets.
        let mut args = vec![
            "uninstall".to_string(),
            scope.flag().into(),
            "--noninteractive".into(),
            "--assumeyes".into(),
            "--no-related".into(),
            "--".into(),
        ];
        args.extend(expected.iter().map(|r| r.reference.clone()));
        let refs: Vec<_> = args.iter().map(String::as_str).collect();
        let out = if scope == FlatpakInstallation::User {
            process::run("flatpak", &args).await?
        } else {
            process::run_privileged("flatpak", &refs, "remove unused Flatpak runtimes").await?
        };
        out.ensure_success("Flatpak cleanup")
    }
}
