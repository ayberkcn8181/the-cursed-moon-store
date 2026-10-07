use std::{fs, os::unix::fs::PermissionsExt};
#[tokio::test]
async fn cache_download_never_calls_live_pacman() {
    let root = std::env::temp_dir().join(format!("tcms-download-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    for(name,script)in[("checkupdates","#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$TCMS_DOWNLOAD_TEST/calls\"\ncase \"$*\" in\n--nocolor) printf 'example 1 -> 2\\n';;\n'--download --nocolor') exit 0;;\n*) exit 51;;\nesac\n"),("pkexec","#!/bin/sh\nexec \"$@\"\n"),("pacman","#!/bin/sh\nexit 55\n")]{let p=root.join(name);fs::write(&p,script).unwrap();fs::set_permissions(p,fs::Permissions::from_mode(0o755)).unwrap();}
    let old = std::env::var("PATH").unwrap();
    std::env::set_var("PATH", format!("{}:{old}", root.display()));
    std::env::set_var("TCMS_DOWNLOAD_TEST", &root);
    if tcms_core::resolve_program("pkexec") == root.join("pkexec")
        && tcms_core::resolve_program("checkupdates") == root.join("checkupdates")
    {
        tcms_pacman::PacmanBackend::default()
            .download_updates()
            .await
            .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("calls")).unwrap(),
            "--nocolor\n--download --nocolor\n"
        );
    }
    std::env::set_var("PATH", old);
    std::env::remove_var("TCMS_DOWNLOAD_TEST");
    fs::remove_dir_all(root).unwrap();
}
