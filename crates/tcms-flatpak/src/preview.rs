//! Resolve dependencies and abort before authentication or deployment.
use libflatpak::{prelude::*, Installation, Transaction, TransactionOperationType};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use tcms_core::{
    Error, FlatpakInstallation, FlatpakRef, PackageAction, PackageId, PackageSource, PreviewEntry,
    Result, TransactionPreview,
};
pub(super) fn resolve(
    scope: FlatpakInstallation,
    action: PackageAction,
    id: Option<PackageId>,
) -> Result<TransactionPreview> {
    let result = || -> std::result::Result<TransactionPreview, anyhow::Error> {
        let installation = match scope {
            FlatpakInstallation::User => Installation::new_user(gio::Cancellable::NONE)?,
            FlatpakInstallation::System => Installation::new_system(gio::Cancellable::NONE)?,
        };
        installation.set_no_interaction(true);
        let installed: HashMap<_, _> = installation
            .list_installed_refs(gio::Cancellable::NONE)?
            .into_iter()
            .filter_map(|r| {
                Some((
                    r.format_ref()?.to_string(),
                    (r.installed_size(), r.commit().map(|v| v.to_string())),
                ))
            })
            .collect();
        if id.is_none() && installed.is_empty() {
            return Ok(TransactionPreview::default());
        }
        let transaction = Transaction::for_installation(&installation, gio::Cancellable::NONE)?;
        transaction.set_no_interaction(true);
        transaction.connect_add_new_remote(|_, _, _, _, _| false);
        transaction.connect_local("choose-remote-for-ref", false, |values| {
            let choice = match values.get(3).and_then(|v| v.get::<Vec<String>>().ok()) {
                Some(r) if r.len() == 1 => 0i32,
                _ => -1i32,
            };
            Some(choice.to_value())
        });
        if let Some(id) = id {
            let reference = id.flatpak_ref()?;
            match action {
                PackageAction::Install => transaction.add_install(
                    &id.flatpak.as_ref().unwrap().origin,
                    &reference,
                    &[],
                )?,
                PackageAction::Update => transaction.add_update(&reference, &[], None)?,
                PackageAction::Remove => transaction.add_uninstall(&reference)?,
            }
        } else {
            for reference in installed.keys() {
                transaction.add_update(reference, &[], None)?;
            }
        }
        let captured = Rc::new(RefCell::new(None));
        let output = captured.clone();
        transaction.connect_ready_pre_auth(move |tx| {
            let collect = || -> std::result::Result<TransactionPreview, anyhow::Error> {
                let mut p = TransactionPreview::default();
                for op in tx.operations().into_iter().filter(|o| !o.is_skipped()) {
                    let reference = op
                        .get_ref()
                        .ok_or_else(|| anyhow::anyhow!("Unresolved Flatpak operation"))?;
                    let parts: Vec<_> = reference.split('/').collect();
                    if parts.len() != 4 {
                        anyhow::bail!("Invalid Flatpak operation reference");
                    }
                    let old = installed.get(reference.as_str());
                    let action = match op.operation_type() {
                        TransactionOperationType::Uninstall => PackageAction::Remove,
                        TransactionOperationType::Update => PackageAction::Update,
                        _ => PackageAction::Install,
                    };
                    let new_size = if action == PackageAction::Remove {
                        0
                    } else {
                        op.installed_size()
                    };
                    p.entries.push(PreviewEntry {
                        id: PackageId {
                            source: PackageSource::Flatpak,
                            id: parts[1].into(),
                            flatpak: Some(FlatpakRef {
                                kind: parts[0].into(),
                                arch: parts[2].into(),
                                branch: parts[3].into(),
                                origin: op.remote().map(|r| r.to_string()).unwrap_or_default(),
                                installation: scope.clone(),
                            }),
                        },
                        action,
                        old_version: old.and_then(|(_, v)| v.clone()),
                        new_version: if action == PackageAction::Remove {
                            None
                        } else {
                            op.commit().map(|v| v.to_string())
                        },
                        download_bytes: Some(op.download_size()),
                        disk_delta: Some(
                            i128::from(new_size) - i128::from(old.map_or(0, |(s, _)| *s)),
                        ),
                    });
                }
                Ok(p)
            };
            *output.borrow_mut() = Some(collect());
            false
        });
        transaction.connect_ready(|_| false);
        let result = transaction.run(gio::Cancellable::NONE);
        if let Some(p) = captured.borrow_mut().take() {
            return p;
        }
        result?;
        anyhow::bail!("Flatpak did not produce a preview")
    };
    result().map_err(|e| Error::Message(format!("Flatpak preview: {e}")))
}
