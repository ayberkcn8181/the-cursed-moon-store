//! Opt-in real pacman smoke test, run as root only inside the Arch CI container.
use std::{fs, path::Path, process::Command};
use tcms_core::{Backend, InstallState, PackageId, PackageSource};

fn checked(program: &str, args: &[&str]) {
    let output = Command::new(program).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{program}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
#[ignore = "requires root in an Arch container; explicitly enable TCMS_ARCH_SMOKE=1"]
async fn real_pacman_installs_queries_and_removes_an_isolated_fixture() {
    assert_eq!(std::env::var("TCMS_ARCH_SMOKE").as_deref(), Ok("1"));
    let root = std::env::temp_dir().join(format!("tcms-arch-smoke-{}", std::process::id()));
    assert!(!root.exists(), "refusing to reuse a previous test root");
    fs::create_dir_all(root.join("root/var/lib/pacman")).unwrap();
    fs::create_dir_all(root.join("package/usr/share/tcms-smoke")).unwrap();
    fs::write(root.join("package/.PKGINFO"), "pkgname = tcms-smoke\npkgbase = tcms-smoke\npkgver = 1.0-1\npkgdesc = Isolated TCMS test fixture\nbuilddate = 1\npackager = TCMS CI\nsize = 8\narch = any\nlicense = MIT\n").unwrap();
    fs::write(
        root.join("package/usr/share/tcms-smoke/marker"),
        "fixture\n",
    )
    .unwrap();
    let archive = root.join("tcms-smoke-1.0-1-any.pkg.tar.gz");
    checked(
        "bsdtar",
        &[
            "-czf",
            archive.to_str().unwrap(),
            "-C",
            root.join("package").to_str().unwrap(),
            ".PKGINFO",
            "usr",
        ],
    );
    let conf = root.join("pacman.conf");
    fs::write(&conf, format!("[options]\nRootDir = {}/root\nDBPath = {}/root/var/lib/pacman\nLogFile = {}/pacman.log\nCacheDir = {}\nArchitecture = auto\nSigLevel = Never\n", root.display(), root.display(), root.display(), root.display())).unwrap();
    let conf = conf.to_str().unwrap();
    checked(
        "pacman",
        &[
            "--config",
            conf,
            "-U",
            "--noconfirm",
            "--nodeps",
            "--noscriptlet",
            archive.to_str().unwrap(),
        ],
    );
    assert!(root.join("root/usr/share/tcms-smoke/marker").is_file());
    assert!(!Path::new("/usr/share/tcms-smoke/marker").exists());
    let backend = tcms_pacman::PacmanBackend::new(true, conf, "");
    let package = backend
        .get_package(&PackageId::new(PackageSource::Pacman, "tcms-smoke"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(package.version, "1.0-1");
    assert_eq!(package.state, InstallState::Installed);
    checked(
        "pacman",
        &[
            "--config",
            conf,
            "-R",
            "--noconfirm",
            "--noscriptlet",
            "tcms-smoke",
        ],
    );
    assert!(!root.join("root/usr/share/tcms-smoke/marker").exists());
    fs::remove_dir_all(root).unwrap();
}
