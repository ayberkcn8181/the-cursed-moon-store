//! Bounded extraction into a private directory. Links are created only after
//! every ordinary entry has been written, and confined to the tool's final root.

use std::collections::HashMap;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};
use cap_std::fs::{Dir, OpenOptions, Permissions, PermissionsExt};
use flate2::read::GzDecoder;
use xz2::{read::XzDecoder, stream::Stream};

#[derive(Clone, Copy)]
pub(crate) enum Compression {
    Gzip,
    Xz,
}

#[derive(Clone, Copy)]
struct Limits {
    expanded: u64,
    file: u64,
    entries: usize,
    metadata: u64,
}

const LIMITS: Limits = Limits {
    expanded: 8 * 1024 * 1024 * 1024,
    file: 4 * 1024 * 1024 * 1024,
    entries: 100_000,
    metadata: 64 * 1024,
};
const MAX_METADATA_TOTAL: u64 = 8 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 4096;
const MAX_PATH_DEPTH: usize = 128;
const MAX_LINK_DEPTH: usize = 64;

// A plain `take` silently returns EOF at its limit, which a tar parser could
// accept as successful completion. Exceeding this budget must instead fail.
struct Bounded<R> {
    inner: R,
    remaining: u64,
}

impl<R: Read> Read for Bounded<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            let mut probe = [0];
            return match self.inner.read(&mut probe)? {
                0 => Ok(0),
                _ => Err(io::Error::other("archive exceeds expanded size limit")),
            };
        }
        let length = output
            .len()
            .min(self.remaining.min(usize::MAX as u64) as usize);
        let read = self.inner.read(&mut output[..length])?;
        self.remaining -= read as u64;
        Ok(read)
    }
}

fn reader(stage: &Dir, compression: Compression) -> Result<Box<dyn Read>> {
    let file = stage.open("artifact")?.into_std();
    Ok(match compression {
        Compression::Gzip => Box::new(GzDecoder::new(file)),
        Compression::Xz => Box::new(XzDecoder::new_stream(
            file,
            Stream::new_stream_decoder(256 * 1024 * 1024, 0)?,
        )),
    })
}

pub(crate) fn extract(stage: &Dir, compression: Compression, marker: &str) -> Result<PathBuf> {
    // Raw preflight bounds extension headers before tar's normal iterator can
    // allocate GNU/PAX metadata. Reopening avoids storing a second expanded tar.
    preflight(reader(stage, compression)?, LIMITS)?;
    stage.create_dir("payload")?;
    let destination = stage.open_dir("payload")?;
    let root = extract_tar(reader(stage, compression)?, &destination, marker, LIMITS)?;
    Ok(Path::new("payload").join(root))
}

fn preflight(reader: impl Read, limits: Limits) -> Result<()> {
    let mut archive = tar::Archive::new(Bounded {
        inner: reader,
        remaining: limits.expanded,
    });
    let mut metadata_total = 0;
    for (index, item) in archive.entries()?.raw(true).enumerate() {
        if index >= limits.entries {
            bail!("archive exceeds entry count limit");
        }
        let mut entry = item?;
        let kind = entry.header().entry_type();
        let size = entry.size();
        if kind.is_gnu_longname() || kind.is_gnu_longlink() || kind.is_pax_local_extensions() {
            if size > limits.metadata {
                bail!("archive extension exceeds metadata limit");
            }
            metadata_total += size;
            if metadata_total > MAX_METADATA_TOTAL {
                bail!("archive exceeds total metadata limit");
            }
            if kind.is_pax_local_extensions() {
                if let Some(extensions) = entry.pax_extensions()? {
                    for extension in extensions {
                        let extension = extension?;
                        let key = extension.key()?;
                        // Size overrides give raw and normal iterators different
                        // boundaries. Sparse files and global PAX are unsupported.
                        if key == "size" || key.starts_with("GNU.sparse.") {
                            bail!("archive contains unsupported PAX size or sparse metadata");
                        }
                    }
                }
            }
        } else if kind.is_file() {
            if size > limits.file {
                bail!("archive file exceeds size limit");
            }
        } else if kind.is_dir() || kind.is_symlink() || kind.is_hard_link() {
            if size != 0 {
                bail!("archive contains data on a non-file entry");
            }
        } else {
            bail!("archive contains an unsupported entry type");
        }
        // Consume entries explicitly so truncated data and decoder errors fail.
        io::copy(&mut entry, &mut io::sink())?;
    }
    // Validate compression trailers too, including bytes following tar's EOF.
    io::copy(&mut archive.into_inner(), &mut io::sink())?;
    Ok(())
}

#[derive(Debug)]
enum Node {
    Directory,
    File,
    HardLink(PathBuf),
    Symlink(PathBuf),
}

fn clean_path(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().len() > MAX_PATH_BYTES || path.components().count() > MAX_PATH_DEPTH {
        bail!("archive path exceeds length limit");
    }
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            _ => bail!("archive contains an unsafe path"),
        }
    }
    Ok(clean)
}

fn insert_node(
    nodes: &mut HashMap<PathBuf, Node>,
    path: PathBuf,
    node: Node,
    limit: usize,
) -> Result<()> {
    if nodes.len() >= limit {
        bail!("archive exceeds expanded entry count limit");
    }
    nodes.insert(path, node);
    Ok(())
}

fn ensure_parents(
    destination: &Dir,
    nodes: &mut HashMap<PathBuf, Node>,
    path: &Path,
    limit: usize,
) -> Result<()> {
    let mut parents: Vec<_> = path
        .ancestors()
        .skip(1)
        .filter(|p| !p.as_os_str().is_empty())
        .collect();
    parents.reverse();
    for parent in parents {
        match nodes.get(parent) {
            Some(Node::Directory) => {}
            Some(_) => bail!("archive entry has a non-directory ancestor"),
            None => {
                insert_node(nodes, parent.to_path_buf(), Node::Directory, limit)?;
                destination.create_dir(parent)?;
            }
        }
    }
    Ok(())
}

fn extract_tar(
    reader: impl Read,
    destination: &Dir,
    marker: &str,
    limits: Limits,
) -> Result<PathBuf> {
    let mut archive = tar::Archive::new(Bounded {
        inner: reader,
        remaining: limits.expanded,
    });
    let mut nodes = HashMap::new();
    for (index, item) in archive.entries()?.enumerate() {
        if index >= limits.entries {
            bail!("archive exceeds entry count limit");
        }
        let mut entry = item?;
        let path = clean_path(&entry.path()?)?;
        let kind = entry.header().entry_type();
        if path.as_os_str().is_empty() {
            if kind.is_dir() {
                continue;
            }
            bail!("archive contains an empty file path");
        }
        ensure_parents(destination, &mut nodes, &path, limits.entries)?;
        if let Some(existing) = nodes.get(&path) {
            if kind.is_dir() && matches!(existing, Node::Directory) {
                continue;
            }
            bail!("archive contains a duplicate or conflicting path");
        }
        let node = if kind.is_dir() {
            destination.create_dir(&path)?;
            Node::Directory
        } else if kind.is_file() {
            if entry.size() > limits.file {
                bail!("archive file exceeds size limit");
            }
            let mut file =
                destination.open_with(&path, OpenOptions::new().write(true).create_new(true))?;
            let expected = entry.size();
            if io::copy(&mut entry, &mut file)? != expected {
                bail!("truncated archive file");
            }
            // Keep executable bits, never restore ownership or special mode bits.
            file.set_permissions(Permissions::from_mode(entry.header().mode()? & 0o777))?;
            Node::File
        } else if kind.is_hard_link() || kind.is_symlink() {
            let target = entry
                .link_name()?
                .context("archive link has no target")?
                .into_owned();
            if target.as_os_str().is_empty() || target.as_os_str().len() > MAX_PATH_BYTES {
                bail!("archive link has an invalid target");
            }
            if kind.is_hard_link() {
                Node::HardLink(clean_path(&target)?)
            } else {
                Node::Symlink(target)
            }
        } else {
            bail!("archive contains an unsupported entry type");
        };
        insert_node(&mut nodes, path, node, limits.entries)?;
    }
    let root = tool_root(&nodes, marker)?;
    // Resolve hard links directly to regular files, including forward chains.
    for (path, node) in &nodes {
        if let Node::HardLink(target) = node {
            let target = hard_link_target(&nodes, target)?;
            if !target.starts_with(&root) {
                bail!("hard link leaves the tool directory");
            }
            destination.hard_link(target, destination, path)?;
        }
    }
    for (path, node) in &nodes {
        if let Node::Symlink(target) = node {
            validate_symlink(path.strip_prefix(&root)?, target)?;
            destination.symlink(target, path)?;
        }
    }
    let tool = destination.open_dir(if root.as_os_str().is_empty() {
        Path::new(".")
    } else {
        &root
    })?;
    // Check actual filesystem resolution: lexical checks alone miss symlink
    // chains combined with '..'. Requiring live targets also catches cycles.
    for (path, node) in &nodes {
        if matches!(node, Node::Symlink(_)) {
            tool.canonicalize(path.strip_prefix(&root)?)
                .with_context(|| format!("unsafe or dangling archive link: {}", path.display()))?;
        }
    }
    if !tool.metadata(marker)?.is_file() {
        bail!("archive tool marker is not a file");
    }
    Ok(root)
}

fn tool_root(nodes: &HashMap<PathBuf, Node>, marker: &str) -> Result<PathBuf> {
    if nodes.contains_key(Path::new(marker)) {
        return Ok(PathBuf::new());
    }
    let first = nodes.keys().next().context("archive is empty")?;
    let root = PathBuf::from(
        first
            .components()
            .next()
            .context("archive is empty")?
            .as_os_str(),
    );
    if !matches!(nodes.get(&root), Some(Node::Directory))
        || nodes.keys().any(|path| !path.starts_with(&root))
        || !nodes.contains_key(&root.join(marker))
    {
        bail!("archive does not contain one valid compatibility tool");
    }
    Ok(root)
}

fn hard_link_target<'a>(
    nodes: &'a HashMap<PathBuf, Node>,
    mut target: &'a Path,
) -> Result<&'a Path> {
    for _ in 0..MAX_LINK_DEPTH {
        match nodes.get(target) {
            Some(Node::File) => return Ok(target),
            Some(Node::HardLink(next)) => target = next,
            _ => bail!("hard link target is not an archived regular file"),
        }
    }
    bail!("archive hard link chain is cyclic or too long")
}

fn validate_symlink(path: &Path, target: &Path) -> Result<()> {
    let mut depth = path
        .parent()
        .context("invalid symlink path")?
        .components()
        .count();
    if target.components().count() > MAX_PATH_DEPTH {
        bail!("archive link target is too deep");
    }
    for component in target.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => bail!("symbolic link leaves the tool directory"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{staging::Stage, test_support::TempRoot};
    use std::io::Write;
    use std::os::unix::fs::{MetadataExt, PermissionsExt as _};
    use tar::{Builder, EntryType, Header};

    type Item<'a> = (&'a str, EntryType, &'a str, &'a [u8]);

    fn append(builder: &mut Builder<Vec<u8>>, (path, kind, target, data): Item<'_>) {
        let mut header = Header::new_gnu();
        header.set_entry_type(kind);
        header.set_mode(0o6755);
        header.set_size(data.len() as u64);
        if !target.is_empty() {
            header.set_link_name(target).unwrap();
        }
        // Write raw header paths so malicious fixtures aren't sanitized by Builder.
        if path.starts_with('/') || path.split('/').any(|part| part == "..") {
            header.as_mut_bytes()[..path.len()].copy_from_slice(path.as_bytes());
            header.set_cksum();
            builder.append(&header, data).unwrap();
        } else {
            builder.append_data(&mut header, path, data).unwrap();
        }
    }

    fn tar(items: &[Item<'_>]) -> Vec<u8> {
        let mut builder = Builder::new(Vec::new());
        for item in items {
            append(&mut builder, *item);
        }
        builder.into_inner().unwrap()
    }

    fn unpack(bytes: &[u8], destination: &Dir, marker: &str, limits: Limits) -> Result<PathBuf> {
        preflight(bytes, limits)?;
        extract_tar(bytes, destination, marker, limits)
    }

    fn rejects(items: &[Item<'_>], expected: &str) {
        let root = TempRoot::new();
        let dir = Dir::open_ambient_dir(&root.0, cap_std::ambient_authority()).unwrap();
        let error = unpack(&tar(items), &dir, "bin/wine", LIMITS).unwrap_err();
        assert!(format!("{error:#}").contains(expected), "{error:#}");
    }

    #[test]
    fn accepts_relative_symlinks_forward_hard_links_and_executable_modes() {
        let root = TempRoot::new();
        let dir = Dir::open_ambient_dir(&root.0, cap_std::ambient_authority()).unwrap();
        let bytes = tar(&[
            ("./tool/bin/wine", EntryType::Symlink, "../lib/wine", b""),
            ("tool/lib/alias", EntryType::Link, "./tool/lib/chain", b""),
            ("tool/lib/chain", EntryType::Link, "tool/lib/wine", b""),
            ("tool/lib/wine", EntryType::Regular, "", b"executable"),
            ("tool/lib", EntryType::Directory, "", b""),
        ]);
        assert_eq!(
            unpack(&bytes, &dir, "bin/wine", LIMITS).unwrap(),
            Path::new("tool")
        );
        assert_eq!(
            std::fs::read(root.0.join("tool/bin/wine")).unwrap(),
            b"executable"
        );
        let file = std::fs::metadata(root.0.join("tool/lib/wine")).unwrap();
        assert_eq!(file.permissions().mode() & 0o7777, 0o755);
        assert_eq!(
            file.ino(),
            std::fs::metadata(root.0.join("tool/lib/alias"))
                .unwrap()
                .ino()
        );
    }

    #[test]
    fn accepts_root_level_tools_and_directory_symlinks() {
        let root = TempRoot::new();
        let dir = Dir::open_ambient_dir(&root.0, cap_std::ambient_authority()).unwrap();
        let bytes = tar(&[
            (".", EntryType::Directory, "", b""),
            ("bin/wine", EntryType::Regular, "", b"wine"),
            ("bin/again", EntryType::Symlink, "wine", b""),
            ("bin/chain", EntryType::Symlink, "again", b""),
            ("alternate", EntryType::Symlink, "bin", b""),
        ]);
        assert_eq!(
            unpack(&bytes, &dir, "bin/wine", LIMITS).unwrap(),
            Path::new("")
        );
        assert_eq!(dir.read("alternate/chain").unwrap(), b"wine");
    }

    #[test]
    fn rejects_traversal_and_absolute_entry_paths() {
        for path in ["../escape", "/tmp/tcms-escape", "tool/../escape"] {
            rejects(&[(path, EntryType::Regular, "", b"bad")], "unsafe path");
        }
    }

    #[test]
    fn rejects_symlinks_outside_the_final_tool_root() {
        for target in ["../../artifact", "/etc/passwd", "../../tool/bin/wine"] {
            rejects(
                &[
                    ("tool/bin/wine", EntryType::Regular, "", b"wine"),
                    ("tool/bin/link", EntryType::Symlink, target, b""),
                ],
                "leaves the tool directory",
            );
        }
    }

    #[test]
    fn checks_real_symlink_resolution_as_well_as_lexical_paths() {
        rejects(
            &[
                ("tool/bin/wine", EntryType::Regular, "", b"wine"),
                ("tool/bin/alias", EntryType::Symlink, "..", b""),
                (
                    "tool/bin/escape",
                    EntryType::Symlink,
                    "alias/../artifact",
                    b"",
                ),
            ],
            "unsafe or dangling",
        );
    }

    #[test]
    fn rejects_cyclic_and_dangling_links() {
        for kind in [EntryType::Symlink, EntryType::Link] {
            let prefix = if kind.is_hard_link() { "tool/" } else { "" };
            for second_target in [format!("{prefix}a"), format!("{prefix}missing")] {
                let root = TempRoot::new();
                let dir = Dir::open_ambient_dir(&root.0, cap_std::ambient_authority()).unwrap();
                let first_target = format!("{prefix}b");
                let bytes = tar(&[
                    ("tool/bin/wine", EntryType::Regular, "", b"wine"),
                    ("tool/a", kind, &first_target, b""),
                    ("tool/b", kind, &second_target, b""),
                ]);
                assert!(unpack(&bytes, &dir, "bin/wine", LIMITS).is_err());
            }
        }
    }

    #[test]
    fn hard_links_must_target_archived_regular_files() {
        for target in [
            "../artifact",
            "/etc/passwd",
            "artifact",
            "tool/bin",
            "tool/link",
        ] {
            let root = TempRoot::new();
            let dir = Dir::open_ambient_dir(&root.0, cap_std::ambient_authority()).unwrap();
            let bytes = tar(&[
                ("tool/bin/wine", EntryType::Regular, "", b"wine"),
                ("tool/link", EntryType::Symlink, "bin/wine", b""),
                ("tool/hard", EntryType::Link, target, b""),
            ]);
            assert!(
                unpack(&bytes, &dir, "bin/wine", LIMITS).is_err(),
                "{target}"
            );
        }
    }

    #[test]
    fn rejects_writes_through_link_ancestors_in_either_order() {
        for reverse in [false, true] {
            let mut items = vec![
                ("tool/dir", EntryType::Symlink, "bin", &b""[..]),
                ("tool/dir/file", EntryType::Regular, "", &b"bad"[..]),
            ];
            if reverse {
                items.reverse();
            }
            let root = TempRoot::new();
            let dir = Dir::open_ambient_dir(&root.0, cap_std::ambient_authority()).unwrap();
            assert!(unpack(&tar(&items), &dir, "bin/wine", LIMITS).is_err());
            assert!(!dir.exists("tool/bin/file"));
        }
    }

    #[test]
    fn rejects_duplicate_files_and_multiple_tool_roots() {
        rejects(
            &[
                ("tool/bin/wine", EntryType::Regular, "", b"one"),
                ("tool/bin/wine", EntryType::Regular, "", b"two"),
            ],
            "duplicate",
        );
        rejects(
            &[
                ("tool/bin/wine", EntryType::Regular, "", b"one"),
                ("extra", EntryType::Regular, "", b"two"),
            ],
            "one valid compatibility tool",
        );
    }

    #[test]
    fn rejects_special_entries_and_non_file_data() {
        for kind in [
            EntryType::Fifo,
            EntryType::Char,
            EntryType::Block,
            EntryType::GNUSparse,
            EntryType::XGlobalHeader,
        ] {
            assert!(preflight(&tar(&[("device", kind, "", b"")])[..], LIMITS).is_err());
        }
        assert!(preflight(
            &tar(&[("dir", EntryType::Directory, "", b"bad")])[..],
            LIMITS
        )
        .is_err());
    }

    #[test]
    fn enforces_file_expansion_entry_and_metadata_budgets() {
        let bytes = tar(&[("bin/wine", EntryType::Regular, "", b"12345")]);
        for limits in [
            Limits { file: 4, ..LIMITS },
            Limits {
                expanded: 512,
                ..LIMITS
            },
            Limits {
                entries: 0,
                ..LIMITS
            },
        ] {
            assert!(preflight(&bytes[..], limits).is_err());
        }
        let metadata = tar(&[("long", EntryType::GNULongName, "", b"tool/bin/wine\0")]);
        assert!(preflight(
            &metadata[..],
            Limits {
                metadata: 4,
                ..LIMITS
            }
        )
        .is_err());
        let root = TempRoot::new();
        let dir = Dir::open_ambient_dir(&root.0, cap_std::ambient_authority()).unwrap();
        // Implicit parent directories count too.
        assert!(unpack(
            &bytes,
            &dir,
            "bin/wine",
            Limits {
                entries: 1,
                ..LIMITS
            }
        )
        .is_err());
    }

    #[test]
    fn supports_gnu_long_paths_and_pax_paths_but_rejects_size_overrides() {
        for pax in [false, true] {
            let long = format!("tool/{}/file", "directory".repeat(20));
            let mut builder = Builder::new(Vec::new());
            append(
                &mut builder,
                ("tool/bin/wine", EntryType::Regular, "", b"wine"),
            );
            if pax {
                builder
                    .append_pax_extensions([("path", long.as_bytes())])
                    .unwrap();
            }
            append(
                &mut builder,
                (
                    if pax { "ignored" } else { &long },
                    EntryType::Regular,
                    "",
                    b"long",
                ),
            );
            let bytes = builder.into_inner().unwrap();
            let root = TempRoot::new();
            let dir = Dir::open_ambient_dir(&root.0, cap_std::ambient_authority()).unwrap();
            unpack(&bytes, &dir, "bin/wine", LIMITS).unwrap();
            assert_eq!(dir.read(&long).unwrap(), b"long");
        }
        for key in ["size", "GNU.sparse.size"] {
            let mut builder = Builder::new(Vec::new());
            builder.append_pax_extensions([(key, &b"1"[..])]).unwrap();
            append(&mut builder, ("bin/wine", EntryType::Regular, "", b"w"));
            let bytes = builder.into_inner().unwrap();
            assert!(preflight(&bytes[..], LIMITS)
                .unwrap_err()
                .to_string()
                .contains("unsupported PAX"));
        }
    }

    #[test]
    fn compressed_archives_extract_publish_and_keep_internal_links() {
        let bytes = tar(&[
            ("tool/bin/wine", EntryType::Symlink, "wine64", b""),
            ("tool/bin/wine64", EntryType::Regular, "", b"wine"),
        ]);
        for compression in [Compression::Gzip, Compression::Xz] {
            let root = TempRoot::new();
            let stage = Stage::create(&root.0).unwrap();
            let file = stage.dir.create("artifact").unwrap();
            match compression {
                Compression::Gzip => {
                    let mut encoder =
                        flate2::write::GzEncoder::new(file, flate2::Compression::fast());
                    encoder.write_all(&bytes).unwrap();
                    encoder.finish().unwrap();
                }
                Compression::Xz => {
                    let mut encoder = xz2::write::XzEncoder::new(file, 1);
                    encoder.write_all(&bytes).unwrap();
                    encoder.finish().unwrap();
                }
            }
            let extracted = extract(&stage.dir, compression, "bin/wine").unwrap();
            stage.publish(&extracted, "v1").unwrap();
            drop(stage);
            assert_eq!(std::fs::read(root.0.join("v1/bin/wine")).unwrap(), b"wine");
            assert_eq!(std::fs::read_dir(&root.0).unwrap().count(), 1);
        }
    }

    #[test]
    fn rejects_truncated_tar_and_corrupt_compression_trailers() {
        let bytes = tar(&[("bin/wine", EntryType::Regular, "", b"wine")]);
        let root = TempRoot::new();
        let dir = Dir::open_ambient_dir(&root.0, cap_std::ambient_authority()).unwrap();
        assert!(unpack(&bytes[..513], &dir, "bin/wine", LIMITS).is_err());
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&bytes).unwrap();
        let mut gzip = encoder.finish().unwrap();
        *gzip.last_mut().unwrap() ^= 0xff;
        dir.write("artifact", gzip).unwrap();
        assert!(extract(&dir, Compression::Gzip, "bin/wine").is_err());
        assert!(!dir.exists("payload"));
    }
}
