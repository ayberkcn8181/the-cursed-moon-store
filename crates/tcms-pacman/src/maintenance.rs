//! Maintenance previews are revalidated immediately before execution.
use crate::{parse_package_info, PacmanBackend};
use std::path::PathBuf;
use tcms_core::{process, t, Error, PackageId, PackageSource, Result};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Orphan {
    pub name: String,
    pub version: String,
    pub size: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheFile {
    pub path: PathBuf,
    pub size: u64,
    pub modified: std::time::SystemTime,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachePlan {
    pub directories: Vec<String>,
    pub files: Vec<CacheFile>,
}
impl PacmanBackend {
    pub async fn orphans(&self) -> Result<Vec<Orphan>> {
        self.ensure_enabled()?;
        let mut args = self.conf_args();
        args.push("-Qdtq".into());
        let out = process::run("pacman", args).await?;
        if out.status == 1 && out.stdout.trim().is_empty() && out.stderr.trim().is_empty() {
            return Ok(Vec::new());
        }
        out.ensure_success("pacman orphan query")?;
        let names: Vec<_> = out.stdout.lines().filter(|s| !s.is_empty()).collect();
        let mut result = Vec::new();
        for chunk in names.chunks(100) {
            let mut args = self.conf_args();
            args.extend(["-Qi".into(), "--".into()]);
            for name in chunk {
                tcms_core::assert_safe_package_id(&PackageId::new(PackageSource::Pacman, *name))?;
                args.push((*name).into());
            }
            let out = process::run("pacman", args).await?;
            out.ensure_success("pacman orphan metadata")?;
            for block in out.stdout.split("\n\n").filter(|b| !b.trim().is_empty()) {
                let info = parse_package_info(block);
                let name = info
                    .name
                    .ok_or_else(|| Error::Message("Missing orphan name".into()))?;
                if !chunk.contains(&name.as_str()) {
                    return Err(Error::Message("Unexpected orphan metadata".into()));
                }
                result.push(Orphan {
                    name,
                    version: info
                        .version
                        .ok_or_else(|| Error::Message("Missing orphan version".into()))?,
                    size: info.size_bytes,
                });
            }
        }
        result.sort_by(|a, b| a.name.cmp(&b.name));
        if result.len() != names.len() {
            return Err(Error::Message("Incomplete orphan metadata".into()));
        }
        Ok(result)
    }
    pub async fn remove_orphans(&self, expected: &[Orphan]) -> Result<()> {
        if expected.is_empty() {
            return Err(Error::Message("No orphan packages selected".into()));
        }
        if self.orphans().await? != expected {
            return Err(Error::Message(t("management.changed")));
        }
        // No recursive removal: only displayed orphans become targets.
        let args = self.orphan_removal_args(expected);
        let refs: Vec<_> = args.iter().map(String::as_str).collect();
        process::run_privileged_pacman(&refs, "remove orphan packages").await?;
        Ok(())
    }
    fn orphan_removal_args(&self, expected: &[Orphan]) -> Vec<String> {
        let mut args = self.conf_args();
        args.extend(["-R".into(), "--noconfirm".into(), "--".into()]);
        args.extend(expected.iter().map(|p| p.name.clone()));
        args
    }
    pub async fn cache_plan(&self) -> Result<CachePlan> {
        self.ensure_enabled()?;
        let mut args = self.conf_args();
        args.push("CacheDir".into());
        let out = process::run("pacman-conf", args).await?;
        out.ensure_success("pacman cache directories")?;
        let directories: Vec<_> = out
            .stdout
            .lines()
            .filter(|s| !s.trim().is_empty())
            .map(str::to_string)
            .collect();
        if directories.is_empty()
            || directories
                .iter()
                .any(|s| !std::path::Path::new(s).is_absolute())
        {
            return Err(Error::Message("Invalid pacman cache directories".into()));
        }
        let out = process::run("paccache", cache_args(&directories, false)).await?;
        out.ensure_success("paccache preview")?;
        let mut files = Vec::new();
        for name in candidate_paths(&out.stdout)? {
            let path = PathBuf::from(name);
            if !directories
                .iter()
                .any(|d| path.parent() == Some(std::path::Path::new(d)))
            {
                return Err(Error::Message("Unexpected cache preview path".into()));
            }
            let m = path.symlink_metadata()?;
            if !m.is_file() {
                return Err(Error::Message(
                    "Cache candidate is not a regular file".into(),
                ));
            }
            files.push(CacheFile {
                path,
                size: m.len(),
                modified: m.modified()?,
            });
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(CachePlan { directories, files })
    }
    pub async fn clean_cache(&self, expected: &CachePlan) -> Result<()> {
        if expected.files.is_empty() {
            return Err(Error::Message("No cached packages selected".into()));
        }
        if &self.cache_plan().await? != expected {
            return Err(Error::Message(t("management.changed")));
        }
        let args = cache_args(&expected.directories, true);
        let refs: Vec<_> = args.iter().map(String::as_str).collect();
        process::run_privileged("paccache", &refs, "clean old package cache")
            .await?
            .ensure_success("paccache")
    }
}
fn cache_args(directories: &[String], remove: bool) -> Vec<String> {
    let mut args = vec![
        if remove { "--remove" } else { "--dryrun" }.into(),
        "--keep".into(),
        "3".into(),
    ];
    if !remove {
        args.extend([
            "--verbose".into(),
            "--verbose".into(),
            "--null".into(),
            "--nocolor".into(),
        ]);
    }
    for d in directories {
        args.extend(["--cachedir".into(), d.clone()]);
    }
    args
}
fn candidate_paths(output: &str) -> Result<Vec<&str>> {
    if !output.contains('\0') {
        if output.trim().is_empty()
            || output.trim() == "==> no candidate packages found for pruning"
        {
            return Ok(Vec::new());
        }
        return Err(Error::Message("Unexpected paccache preview output".into()));
    }
    let (candidates, footer) = output.rsplit_once('\0').unwrap();
    let footer = footer.trim();
    if !footer.is_empty() && !footer.starts_with("==> finished dry run:") {
        return Err(Error::Message("Unexpected paccache preview footer".into()));
    }
    let candidates = candidates
        .strip_prefix("==> Candidate packages:\n")
        .unwrap_or(candidates);
    let paths: Vec<_> = candidates.split('\0').collect();
    if paths
        .iter()
        .any(|p| !p.starts_with('/') || p.contains('\n'))
    {
        return Err(Error::Message("Invalid cache preview filename".into()));
    }
    Ok(paths)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn orphan_removal_limits_targets_and_preserves_custom_configuration() {
        let backend = PacmanBackend::new(true, "/custom/pacman.conf", "--recursive");
        let orphans = vec![Orphan {
            name: "unused-library".into(),
            version: "1-1".into(),
            size: None,
        }];
        assert_eq!(
            backend.orphan_removal_args(&orphans),
            [
                "--config",
                "/custom/pacman.conf",
                "-R",
                "--noconfirm",
                "--",
                "unused-library"
            ]
        );
    }
    #[test]
    fn parse_candidates_without_treating_status_messages_as_files() {
        assert_eq!(
            candidate_paths("/cache/a.pkg.tar.zst\0/cache/a.pkg.tar.zst.sig\0\n")
                .unwrap()
                .len(),
            2
        );
        assert!(
            candidate_paths("==> no candidate packages found for pruning\n")
                .unwrap()
                .is_empty()
        );
        assert_eq!(candidate_paths("==> Candidate packages:\n/cache/a.pkg.tar.zst\0\n==> finished dry run: 1 candidates (disk space saved: 1 MiB)\n").unwrap(),["/cache/a.pkg.tar.zst"]);
        assert!(candidate_paths("unexpected").is_err());
    }
    #[test]
    fn preview_and_cleanup_keep_three_versions_in_the_same_directories() {
        let dirs = vec!["/var/cache/pacman/pkg".into(), "/other/cache".into()];
        let p = cache_args(&dirs, false);
        let e = cache_args(&dirs, true);
        assert!(p.contains(&"--dryrun".into()));
        assert!(!p.contains(&"--remove".into()));
        assert_eq!(&e[..3], ["--remove", "--keep", "3"]);
        assert_eq!(&p[7..], &e[3..]);
    }
}
