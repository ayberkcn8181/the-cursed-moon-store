use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{bail, Context, Result};
use cap_std::fs::{Dir, DirBuilder, DirBuilderExt};
use rustix::fs::{renameat_with, RenameFlags};

use crate::safety::{safe_component, timestamp};

static NEXT_STAGE: AtomicU64 = AtomicU64::new(0);

/// Owns only a directory created exclusively by this installation attempt.
pub(crate) struct Stage {
    parent: Dir,
    name: String,
    pub(crate) path: PathBuf,
    pub(crate) dir: Dir,
}

impl Stage {
    pub(crate) fn create(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        let parent = Dir::open_ambient_dir(root, cap_std::ambient_authority())?;
        for _ in 0..128 {
            let name = format!(
                ".tcms-stage-{}-{}-{}",
                std::process::id(),
                timestamp(),
                NEXT_STAGE.fetch_add(1, Ordering::Relaxed)
            );
            match parent.create_dir_with(&name, DirBuilder::new().mode(0o700)) {
                Ok(()) => {
                    let dir = match parent.open_dir(&name) {
                        Ok(dir) => dir,
                        Err(error) => {
                            let _ = parent.remove_dir(&name);
                            return Err(error.into());
                        }
                    };
                    return Ok(Self {
                        path: root.join(&name),
                        parent,
                        name,
                        dir,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        bail!("could not create a unique installation directory")
    }

    pub(crate) fn publish(&self, extracted: &Path, version: &str) -> Result<()> {
        let version = safe_component(version)?;
        // Linux renameat2 makes the existence check and publication atomic;
        // an empty directory or symlink belonging to another install is safe.
        renameat_with(
            &self.dir,
            extracted,
            &self.parent,
            version,
            RenameFlags::NOREPLACE,
        )
        .with_context(|| format!("could not install {version}; destination may already exist"))?;
        Ok(())
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        if let Err(error) = self.parent.remove_dir_all(&self.name) {
            tracing::warn!(%error, stage = %self.name, "could not clean installation staging directory");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempRoot;

    #[test]
    fn stages_are_private_unique_and_cleaned_independently() {
        use std::os::unix::fs::PermissionsExt;
        let root = TempRoot::new();
        let first = Stage::create(&root.0).unwrap();
        let second = Stage::create(&root.0).unwrap();
        assert_ne!(first.path, second.path);
        assert_eq!(
            std::fs::metadata(&first.path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let first_path = first.path.clone();
        drop(first);
        assert!(!first_path.exists());
        assert!(second.path.exists());
    }

    #[test]
    fn publication_never_replaces_existing_destinations() {
        let root = TempRoot::new();
        for name in ["empty", "file", "link"] {
            match name {
                "empty" => std::fs::create_dir(root.0.join(name)).unwrap(),
                "file" => std::fs::write(root.0.join(name), "keep").unwrap(),
                _ => std::os::unix::fs::symlink("file", root.0.join(name)).unwrap(),
            }
            let stage = Stage::create(&root.0).unwrap();
            stage.dir.create_dir("payload").unwrap();
            assert!(stage.publish(Path::new("payload"), name).is_err());
            assert!(stage.dir.is_dir("payload"));
        }
        assert!(root.0.join("empty").is_dir());
        assert_eq!(
            std::fs::read_to_string(root.0.join("file")).unwrap(),
            "keep"
        );
        assert_eq!(
            std::fs::read_link(root.0.join("link")).unwrap(),
            Path::new("file")
        );
    }

    #[test]
    fn successful_publication_survives_stage_cleanup() {
        let root = TempRoot::new();
        let stage = Stage::create(&root.0).unwrap();
        stage.dir.create_dir("payload").unwrap();
        stage.dir.write("payload/marker", "ok").unwrap();
        stage.publish(Path::new("payload"), "v1").unwrap();
        drop(stage);
        assert_eq!(
            std::fs::read_to_string(root.0.join("v1/marker")).unwrap(),
            "ok"
        );
        assert_eq!(std::fs::read_dir(&root.0).unwrap().count(), 1);
    }

    #[test]
    fn concurrent_installs_have_exactly_one_winner() {
        let root = TempRoot::new();
        let barrier = std::sync::Barrier::new(2);
        let results = std::thread::scope(|scope| {
            let handles: Vec<_> = ["first", "second"]
                .into_iter()
                .map(|content| {
                    let root = &root.0;
                    let barrier = &barrier;
                    scope.spawn(move || {
                        let stage = Stage::create(root).unwrap();
                        stage.dir.create_dir("payload").unwrap();
                        stage.dir.write("payload/marker", content).unwrap();
                        barrier.wait();
                        stage.publish(Path::new("payload"), "v1").is_ok()
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(results.iter().filter(|success| **success).count(), 1);
        assert!(["first", "second"].contains(
            &std::fs::read_to_string(root.0.join("v1/marker"))
                .unwrap()
                .as_str()
        ));
        assert_eq!(std::fs::read_dir(&root.0).unwrap().count(), 1);
    }

    #[test]
    fn cleanup_does_not_follow_links_outside_staging() {
        let root = TempRoot::new();
        let outside = root.0.join("keep");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("marker"), "keep").unwrap();
        let stage = Stage::create(&root.0).unwrap();
        stage.dir.symlink("../keep", "link").unwrap();
        drop(stage);
        assert_eq!(
            std::fs::read_to_string(outside.join("marker")).unwrap(),
            "keep"
        );
    }
}
