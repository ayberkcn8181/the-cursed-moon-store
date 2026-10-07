//! Exercise backend parsing and transaction argv without touching real Flatpaks.
use std::{fs, os::unix::fs::PermissionsExt};
use tcms_core::{Backend, InstallState, SearchQuery};
use tcms_flatpak::FlatpakBackend;

#[tokio::test]
async fn scoped_refs_survive_search_listing_updates_and_transactions() {
    let root = std::env::temp_dir().join(format!("tcms-flatpak-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let script = root.join("flatpak");
    fs::write(&script, r#"#!/bin/sh
printf '%s\n' "$*" >> "$TCMS_FIXTURE_ROOT/calls"
case "$1" in
--default-arch) printf 'x86_64\n';;
search)
  if [ -f "$TCMS_FIXTURE_ROOT/fail" ]; then echo 'remote unavailable' >&2; exit 42; fi
  printf 'org.example.App\tExample\t1\tDescription\tstable\tflathub\norg.example.App\tExample\t2\tDescription\tbeta\ttesting\n';;
list)
  case " $* " in *' --runtime '*)
    printf 'org.example.Platform\tx86_64\tstable\tflathub\tExample Runtime\t1\tRuntime\n'
    case " $* " in *' --all '*) printf 'org.example.Platform.Locale\tx86_64\tstable\tflathub\tTranslations\t\tLocale extension\n';; esac
    exit 0;; esac
  printf 'org.example.App\tx86_64\tstable\tflathub\tExample\t1\tDescription\norg.example.App\tx86_64\tbeta\ttesting\tExample Beta\t2\tDescription\n';;
remote-ls)
  case " $* " in *' --runtime '*) exit 0;; esac
  printf 'org.example.App\tx86_64\tbeta\ttesting\tExample Beta\t3\tDescription\n';;
install|update|uninstall) exit 0;;
*) exit 9;;
esac
"#).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let old_path = std::env::var_os("PATH").unwrap();
    std::env::set_var(
        "PATH",
        std::env::join_paths(std::iter::once(root.clone()).chain(std::env::split_paths(&old_path)))
            .unwrap(),
    );
    std::env::set_var("TCMS_FIXTURE_ROOT", &root);
    let mut backend = FlatpakBackend::new(true, "user", "flathub|https://example.org");
    let installed = backend.installed().await.unwrap();
    assert_eq!(installed.len(), 4);
    assert_eq!(installed[3].id.id, "org.example.Platform.Locale");
    assert_eq!(installed[2].id.flatpak.as_ref().unwrap().kind, "runtime");
    assert!(installed.iter().all(|p| p.state == InstallState::Installed));
    assert_eq!(
        fs::read_to_string(root.join("calls"))
            .unwrap()
            .lines()
            .count(),
        2,
        "installed list must query local apps and runtimes only"
    );
    let search = backend
        .search(&SearchQuery {
            text: "Example".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    let beta = &search.packages[1].id;
    assert_eq!(*beta, installed[1].id);
    let updates = backend.updates().await.unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].id, *beta);
    assert_eq!(updates[0].available_version.as_deref(), Some("3"));
    backend.set_installation("system");
    let system = backend.installed().await.unwrap();
    assert_eq!(system[0].id.id, installed[0].id.id);
    assert_ne!(system[0].id, installed[0].id);
    assert_eq!(
        system[0].id.flatpak.as_ref().unwrap().installation.label(),
        "system"
    );
    backend.install(beta).await.unwrap();
    backend.update(beta).await.unwrap();
    backend.remove(beta).await.unwrap();
    backend.set_installation("user");
    backend.download_updates().await.unwrap();
    let calls = fs::read_to_string(root.join("calls")).unwrap();
    assert!(calls.contains("update --no-deploy --noninteractive -y --user"));
    assert!(calls
        .contains("install -y --user --noninteractive -- testing app/org.example.App/x86_64/beta"));
    assert!(
        calls.contains("uninstall -y --user --noninteractive -- app/org.example.App/x86_64/beta")
    );
    fs::write(root.join("fail"), "").unwrap();
    let error = backend
        .search(&SearchQuery {
            text: "Example".into(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert!(error.to_string().contains("remote unavailable"));
    std::env::set_var("PATH", old_path);
    std::env::remove_var("TCMS_FIXTURE_ROOT");
    fs::remove_dir_all(root).unwrap();
}
