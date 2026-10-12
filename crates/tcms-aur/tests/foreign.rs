use std::{
    io::{Read, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
};
use tcms_aur::AurBackend;
use tcms_core::{ForeignStatus, PackageSource};
fn server(body: &'static str) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/rpc", listener.local_addr().unwrap());
    let job = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 4096];
        assert!(stream.read(&mut request).unwrap() > 0);
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
    });
    (url, job)
}
#[tokio::test]
async fn distinguishes_aur_manual_and_unverified_without_losing_inventory() {
    let root = std::env::temp_dir().join(format!("tcms-foreign-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let command = root.join("pacman");
    std::fs::write(
        &command,
        "#!/bin/sh\n[ \"$*\" = '-Qm' ] || exit 77\nprintf 'example-aur 1\\nmanual-package 2\\n'\n",
    )
    .unwrap();
    std::fs::set_permissions(command, std::fs::Permissions::from_mode(0o755)).unwrap();
    let old = std::env::var("PATH").unwrap();
    std::env::set_var("PATH", format!("{}:{old}", root.display()));
    let (url, job) = server(r#"{"results":[{"Name":"example-aur","Version":"2"}]}"#);
    let known = AurBackend::new(true, url, "", "")
        .installed_listing()
        .await
        .unwrap();
    job.join().unwrap();
    assert!(known.errors.is_empty());
    assert_eq!(known.packages.len(), 2);
    let aur = known
        .packages
        .iter()
        .find(|p| p.id.id == "example-aur")
        .unwrap();
    assert_eq!(aur.id.source, PackageSource::Aur);
    assert_eq!(aur.foreign_status, Some(ForeignStatus::InAur));
    let local = known
        .packages
        .iter()
        .find(|p| p.id.id == "manual-package")
        .unwrap();
    assert_eq!(local.id.source, PackageSource::Pacman);
    assert_eq!(local.foreign_status, Some(ForeignStatus::Local));
    let (url, job) = server(r#"{"type":"error","error":"rate limited","results":[]}"#);
    let unknown = AurBackend::new(true, url, "", "")
        .installed_listing()
        .await
        .unwrap();
    job.join().unwrap();
    assert_eq!(unknown.errors.len(), 1);
    assert_eq!(unknown.packages.len(), 2);
    assert!(unknown
        .packages
        .iter()
        .all(|p| p.foreign_status == Some(ForeignStatus::Unverified)));
    std::env::set_var("PATH", old);
    std::fs::remove_dir_all(root).unwrap();
}
