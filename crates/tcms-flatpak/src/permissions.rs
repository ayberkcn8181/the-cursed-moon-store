//! Application-scoped overrides. Unchanged controls never emit an override.
use crate::FlatpakBackend;
use std::collections::BTreeSet;
use tcms_core::{process, t, Error, FlatpakInstallation, PackageId, Result};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    Network,
    Audio,
    Wayland,
    X11,
    Gpu,
    Home,
    Host,
    Documents,
    Downloads,
}
impl Permission {
    pub const ALL: [Self; 9] = [
        Self::Network,
        Self::Audio,
        Self::Wayland,
        Self::X11,
        Self::Gpu,
        Self::Home,
        Self::Host,
        Self::Documents,
        Self::Downloads,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::Network => "permissions.network",
            Self::Audio => "permissions.audio",
            Self::Wayland => "permissions.wayland",
            Self::X11 => "permissions.x11",
            Self::Gpu => "permissions.gpu",
            Self::Home => "permissions.home",
            Self::Host => "permissions.host",
            Self::Documents => "permissions.documents",
            Self::Downloads => "permissions.downloads",
        }
    }
    fn argument(self, allow: bool) -> String {
        let (yes, no, value) = match self {
            Self::Network => ("share", "unshare", "network"),
            Self::Audio => ("socket", "nosocket", "pulseaudio"),
            Self::Wayland => ("socket", "nosocket", "wayland"),
            Self::X11 => ("socket", "nosocket", "x11"),
            Self::Gpu => ("device", "nodevice", "dri"),
            Self::Home => ("filesystem", "nofilesystem", "home"),
            Self::Host => ("filesystem", "nofilesystem", "host"),
            Self::Documents => ("filesystem", "nofilesystem", "xdg-documents"),
            Self::Downloads => ("filesystem", "nofilesystem", "xdg-download"),
        };
        format!("--{}={value}", if allow { yes } else { no })
    }
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub effective: String,
    pub overrides: String,
}
#[derive(Clone, Debug)]
pub struct Change {
    pub id: PackageId,
    pub expected_overrides: String,
    pub changes: Vec<(Permission, bool)>,
    pub reset: bool,
}
fn validate(id: &PackageId) -> Result<FlatpakInstallation> {
    id.flatpak_ref()?;
    let r = id.flatpak.as_ref().unwrap();
    if r.kind != "app" {
        return Err(Error::Message("Permissions require an application".into()));
    }
    Ok(r.installation.clone())
}
impl Change {
    pub fn arguments(&self) -> Result<Vec<String>> {
        let scope = validate(&self.id)?;
        if (self.reset && !self.changes.is_empty()) || (!self.reset && self.changes.is_empty()) {
            return Err(Error::Message("Invalid permission change".into()));
        }
        let mut args = vec!["override".into(), scope.flag().into()];
        if self.reset {
            args.push("--reset".into());
        } else {
            args.extend(self.changes.iter().map(|(p, a)| p.argument(*a)));
        }
        // APP must always be present: omission would change global permissions.
        args.extend(["--".into(), self.id.id.clone()]);
        Ok(args)
    }
}
impl FlatpakBackend {
    pub async fn permissions(&self, id: &PackageId) -> Result<Snapshot> {
        self.ensure_enabled()?;
        let scope = validate(id)?;
        let reference = id.flatpak_ref()?;
        let effective = process::run(
            "flatpak",
            ["info", scope.flag(), "--show-permissions", "--", &reference],
        )
        .await?;
        effective.ensure_success("Flatpak permissions")?;
        let overrides = process::run(
            "flatpak",
            ["override", scope.flag(), "--show", "--", &id.id],
        )
        .await?;
        overrides.ensure_success("Flatpak application overrides")?;
        Ok(Snapshot {
            effective: effective.stdout,
            overrides: overrides.stdout,
        })
    }
    pub async fn change_permissions(&self, change: &Change) -> Result<()> {
        let args = change.arguments()?;
        let current = self.permissions(&change.id).await?;
        if current.overrides != change.expected_overrides {
            return Err(Error::Message(t("management.changed")));
        }
        let scope = validate(&change.id)?;
        let refs: Vec<_> = args.iter().map(String::as_str).collect();
        let out = if scope == FlatpakInstallation::User {
            process::run("flatpak", &args).await?
        } else {
            process::run_privileged("flatpak", &refs, "change application permissions").await?
        };
        out.ensure_success("Flatpak permissions")
    }
}
fn entries(metadata: &str) -> BTreeSet<String> {
    let mut group = "";
    let mut result = BTreeSet::new();
    for line in metadata.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            group = name;
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if group == "Context" || group.starts_with("Context ") {
            for item in value.split(';').filter(|v| !v.is_empty()) {
                result.insert(format!("{group}: {key}={item}"));
            }
        } else if group.ends_with("Bus Policy")
            || group.starts_with("Policy ")
            || group.starts_with("USB ")
        {
            result.insert(format!("{group}: {key}={value}"));
        }
    }
    result
}
pub fn manifest_changes(old: &str, new: &str) -> Vec<String> {
    let (old, new) = (entries(old), entries(new));
    new.difference(&old)
        .map(|v| format!("+ {v}"))
        .chain(old.difference(&new).map(|v| format!("− {v}")))
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    use tcms_core::{FlatpakRef, PackageSource};
    #[test]
    fn override_commands_always_target_one_app_and_scope() {
        let mut id = PackageId::new(PackageSource::Flatpak, "org.example.App");
        id.flatpak = Some(FlatpakRef {
            kind: "app".into(),
            arch: "x86_64".into(),
            branch: "stable".into(),
            origin: "flathub".into(),
            installation: FlatpakInstallation::User,
        });
        let mut c = Change {
            id,
            expected_overrides: String::new(),
            changes: vec![(Permission::Network, false)],
            reset: false,
        };
        assert_eq!(
            c.arguments().unwrap(),
            [
                "override",
                "--user",
                "--unshare=network",
                "--",
                "org.example.App"
            ]
        );
        c.id.flatpak.as_mut().unwrap().installation = FlatpakInstallation::System;
        c.reset = true;
        c.changes.clear();
        assert_eq!(
            c.arguments().unwrap(),
            ["override", "--system", "--reset", "--", "org.example.App"]
        );
        c.id.id.clear();
        assert!(c.arguments().is_err());
    }
    #[test]
    fn manifest_diff_includes_additions_and_removals_without_order_noise() {
        let old="[Context]\nsockets=wayland;x11;\n[Session Bus Policy]\norg.example.Service=talk\n[Application]\nruntime=old\n";
        let new = "[Context]\nsockets=x11;wayland;\nshared=network;\n[Application]\nruntime=new\n";
        assert_eq!(
            manifest_changes(old, new),
            [
                "+ Context: shared=network",
                "− Session Bus Policy: org.example.Service=talk"
            ]
        );
    }
}
