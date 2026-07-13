use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Orientation, Spinner};
use libadwaita::prelude::*;
use tcms_compatibility::{
    display_path, dxvk, dxvk_releases_channel, heroic, install_dxvk_release, install_proton_ge,
    install_wine_ge, lutris, proton_ge_releases_channel, remove_tool, scan_with_options, steam,
    wine_ge_releases_channel, CompatibilitySnapshot, CompatibilityTool, DiscoveryOptions, Game,
    LauncherId, LauncherInstallation, ToolKind,
};
use tcms_core::i18n::{t, t_args};

use crate::store::UiBridge;
use crate::widgets::page_shell;

pub struct CompatibilityPage {
    pub root: GtkBox,
    content: GtkBox,
    bridge: UiBridge,
}

#[derive(Clone, Copy)]
enum InstallFlavor {
    Proton,
    Wine,
}

impl CompatibilityPage {
    pub fn new(bridge: UiBridge) -> Self {
        let content = GtkBox::new(Orientation::Vertical, 18);
        content.set_hexpand(true);
        content.set_vexpand(true);
        let scroll = gtk4::ScrolledWindow::builder()
            .child(&content)
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vexpand(true)
            .build();
        let root = page_shell(&t("compat.title"), &scroll);
        let page = Self {
            root,
            content,
            bridge,
        };
        page.reload();
        page
    }

    pub fn reload(&self) {
        load_snapshot(&self.content, &self.bridge);
    }
}

fn discovery_options(bridge: &UiBridge) -> DiscoveryOptions {
    let config = bridge.store.config().compatibility;
    DiscoveryOptions {
        auto_detect: config.auto_detect,
        steam_root: configured_path(&config.steam_root),
        steam_flatpak_root: configured_path(&config.steam_flatpak_root),
        lutris_root: configured_path(&config.lutris_root),
        lutris_flatpak_root: configured_path(&config.lutris_flatpak_root),
        heroic_root: configured_path(&config.heroic_root),
        heroic_flatpak_root: configured_path(&config.heroic_flatpak_root),
    }
}

fn configured_path(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(relative) = value.strip_prefix("~/") {
        return dirs::home_dir().map(|home| home.join(relative));
    }
    Some(PathBuf::from(value))
}

fn load_snapshot(content: &GtkBox, bridge: &UiBridge) {
    clear(content);
    let spinner = Spinner::new();
    spinner.set_spinning(true);
    spinner.set_halign(gtk4::Align::Center);
    spinner.set_valign(gtk4::Align::Center);
    spinner.set_vexpand(true);
    content.append(&spinner);

    let options = discovery_options(bridge);
    let (tx, rx) = mpsc::channel();
    if std::thread::Builder::new()
        .name("tcms-compat-scan".into())
        .spawn(move || {
            let _ = tx.send(scan_with_options(&options));
        })
        .is_err()
    {
        render_error(content, &t("compat.worker_failed"));
        return;
    }

    let content = content.clone();
    let bridge = bridge.clone();
    poll_local(rx, move |snapshot| {
        render_snapshot(&content, &bridge, snapshot);
    });
}

fn render_snapshot(content: &GtkBox, bridge: &UiBridge, snapshot: CompatibilitySnapshot) {
    clear(content);
    content.append(&launcher_group(&snapshot));
    if snapshot.installations.is_empty() {
        content.append(
            &libadwaita::StatusPage::builder()
                .icon_name("applications-games-symbolic")
                .title(t("compat.no_launchers"))
                .description(t("compat.no_launchers_desc"))
                .build(),
        );
    }
    for installation in &snapshot.installations {
        content.append(&tools_group(bridge, &snapshot, installation, content));
        content.append(&games_group(bridge, &snapshot, installation, content));
    }
    for warning in snapshot.warnings {
        let row = libadwaita::ActionRow::builder()
            .title(t("compat.warning"))
            .subtitle(warning)
            .build();
        row.add_prefix(&gtk4::Image::from_icon_name("dialog-warning-symbolic"));
        let group = libadwaita::PreferencesGroup::new();
        group.add(&row);
        content.append(&group);
    }
}

fn launcher_group(snapshot: &CompatibilitySnapshot) -> libadwaita::PreferencesGroup {
    let group = libadwaita::PreferencesGroup::builder()
        .title(t("compat.launchers"))
        .description(t("compat.launchers_desc"))
        .build();
    for launcher in [LauncherId::Steam, LauncherId::Lutris, LauncherId::Heroic] {
        let found: Vec<String> = snapshot
            .installations
            .iter()
            .filter(|item| item.launcher == launcher)
            .map(|item| item.kind.to_string())
            .collect();
        let (subtitle, icon) = if found.is_empty() {
            (t("compat.not_detected"), "circle-outline-thick-symbolic")
        } else {
            (
                t_args("compat.detected_as", &[("types", &found.join(", "))]),
                "emblem-ok-symbolic",
            )
        };
        let row = libadwaita::ActionRow::builder()
            .title(launcher.to_string())
            .subtitle(subtitle)
            .build();
        row.add_prefix(&gtk4::Image::from_icon_name(icon));
        group.add(&row);
    }
    group
}

fn tools_group(
    bridge: &UiBridge,
    snapshot: &CompatibilitySnapshot,
    installation: &LauncherInstallation,
    content: &GtkBox,
) -> libadwaita::PreferencesGroup {
    let group = libadwaita::PreferencesGroup::builder()
        .title(t_args(
            "compat.launcher_tools",
            &[
                ("launcher", &installation.launcher.to_string()),
                ("type", &installation.kind.to_string()),
            ],
        ))
        .description(t_args(
            "compat.tool_path",
            &[("path", &display_path(&installation.tool_root))],
        ))
        .build();

    match installation.launcher {
        LauncherId::Steam => group.add(&install_row(
            bridge,
            installation,
            InstallFlavor::Proton,
            content,
        )),
        LauncherId::Lutris => group.add(&install_row(
            bridge,
            installation,
            InstallFlavor::Wine,
            content,
        )),
        LauncherId::Heroic => {
            let mut proton = installation.clone();
            proton.tool_root = installation.tool_root.join("proton");
            group.add(&install_row(
                bridge,
                &proton,
                InstallFlavor::Proton,
                content,
            ));
            let mut wine = installation.clone();
            wine.tool_root = installation.tool_root.join("wine");
            group.add(&install_row(bridge, &wine, InstallFlavor::Wine, content));
        }
    }

    let tools = tools_for_installation(snapshot, installation);
    if tools.is_empty() {
        group.add(
            &libadwaita::ActionRow::builder()
                .title(t("compat.no_custom_tools"))
                .subtitle(t("compat.no_custom_tools_desc"))
                .build(),
        );
    } else {
        for tool in tools {
            group.add(&tool_row(bridge, snapshot, installation, tool, content));
        }
    }
    group
}

fn install_row(
    bridge: &UiBridge,
    installation: &LauncherInstallation,
    flavor: InstallFlavor,
    content: &GtkBox,
) -> libadwaita::ActionRow {
    let (title, description) = match flavor {
        InstallFlavor::Proton => (t("compat.ge_proton"), t("compat.ge_proton_desc")),
        InstallFlavor::Wine => (t("compat.wine_ge"), t("compat.wine_ge_desc")),
    };
    let row = libadwaita::ActionRow::builder()
        .title(title)
        .subtitle(description)
        .build();
    let button = gtk4::Button::builder()
        .label(t("compat.install_latest"))
        .css_classes(["suggested-action", "pill"])
        .build();
    let config = bridge.store.config().compatibility;
    button.set_sensitive(config.allow_artifact_downloads);
    row.add_suffix(&button);
    row.set_activatable_widget(Some(&button));

    let installation = installation.clone();
    let bridge = bridge.clone();
    let content = content.clone();
    button.connect_clicked(move |button| {
        button.set_sensitive(false);
        bridge.set_activity(Some(&t("compat.installing")));
        let runtime = bridge.store.runtime();
        let allow_prerelease = bridge.store.config().compatibility.release_channel == "prerelease";
        let installation = installation.clone();
        let bridge_done = bridge.clone();
        let content_done = content.clone();
        let button_done = button.clone();
        run_action(
            move || {
                runtime.block_on(async {
                    let release = match flavor {
                        InstallFlavor::Proton => proton_ge_releases_channel(1, allow_prerelease)
                            .await?
                            .into_iter()
                            .next()
                            .ok_or_else(|| anyhow::anyhow!("no Proton-GE release available"))?,
                        InstallFlavor::Wine => wine_ge_releases_channel(1, allow_prerelease)
                            .await?
                            .into_iter()
                            .next()
                            .ok_or_else(|| anyhow::anyhow!("no Wine-GE release available"))?,
                    };
                    let name = release.tag_name.clone();
                    match flavor {
                        InstallFlavor::Proton => {
                            install_proton_ge(&installation, &release).await?;
                        }
                        InstallFlavor::Wine => {
                            install_wine_ge(&installation, &release).await?;
                        }
                    }
                    Ok(name)
                })
            },
            move |result| {
                button_done.set_sensitive(true);
                bridge_done.set_activity(None);
                match result {
                    Ok(name) => {
                        bridge_done.toast_msg(&t_args("compat.installed", &[("name", &name)]))
                    }
                    Err(error) => bridge_done.toast_msg(&t_args(
                        "compat.action_failed",
                        &[("error", &error.to_string())],
                    )),
                }
                load_snapshot(&content_done, &bridge_done);
            },
        );
    });
    row
}

fn tool_row(
    bridge: &UiBridge,
    snapshot: &CompatibilitySnapshot,
    installation: &LauncherInstallation,
    tool: &CompatibilityTool,
    content: &GtkBox,
) -> libadwaita::ActionRow {
    let row = libadwaita::ActionRow::builder()
        .title(&tool.name)
        .subtitle(format!("{} · {}", tool.kind, display_path(&tool.path)))
        .build();
    let remove = gtk4::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text(t("action.remove"))
        .css_classes(["flat"])
        .build();
    row.add_suffix(&remove);

    let mut removal_root = installation.clone();
    if installation.launcher == LauncherId::Heroic {
        removal_root.tool_root = match tool.kind {
            ToolKind::Wine => installation.tool_root.join("wine"),
            ToolKind::Proton => installation.tool_root.join("proton"),
            ToolKind::Dxvk => installation.tool_root.clone(),
        };
    }
    let tool = tool.clone();
    let games = snapshot.games.clone();
    let bridge = bridge.clone();
    let content = content.clone();
    remove.connect_clicked(move |button| {
        let dialog = libadwaita::AlertDialog::builder()
            .heading(t("compat.remove_tool"))
            .body(t_args("compat.remove_tool_body", &[("name", &tool.name)]))
            .build();
        dialog.add_response("cancel", &t("action.cancel"));
        dialog.add_response("remove", &t("action.remove"));
        dialog.set_response_appearance("remove", libadwaita::ResponseAppearance::Destructive);
        let installation = removal_root.clone();
        let tool = tool.clone();
        let games = games.clone();
        let bridge_done = bridge.clone();
        let content_done = content.clone();
        let button_done = button.clone();
        dialog.connect_response(None, move |_, response| {
            if response != "remove" {
                return;
            }
            button_done.set_sensitive(false);
            let installation = installation.clone();
            let tool = tool.clone();
            let games = games.clone();
            let bridge_done = bridge_done.clone();
            let content_done = content_done.clone();
            run_action(
                move || {
                    let name = tool.name.clone();
                    remove_tool(&installation, &tool, &games)?;
                    Ok(name)
                },
                move |result| {
                    match result {
                        Ok(name) => {
                            bridge_done.toast_msg(&t_args("compat.removed", &[("name", &name)]))
                        }
                        Err(error) => bridge_done.toast_msg(&t_args(
                            "compat.action_failed",
                            &[("error", &error.to_string())],
                        )),
                    }
                    load_snapshot(&content_done, &bridge_done);
                },
            );
        });
        dialog.present(Some(&bridge.window));
    });
    row
}

fn games_group(
    bridge: &UiBridge,
    snapshot: &CompatibilitySnapshot,
    installation: &LauncherInstallation,
    content: &GtkBox,
) -> libadwaita::PreferencesGroup {
    let group = libadwaita::PreferencesGroup::builder()
        .title(t_args(
            "compat.launcher_games",
            &[
                ("launcher", &installation.launcher.to_string()),
                ("type", &installation.kind.to_string()),
            ],
        ))
        .description(t("compat.launcher_games_desc"))
        .build();
    let games: Vec<_> = snapshot
        .games
        .iter()
        .filter(|game| {
            game.launcher == installation.launcher && game.installation == installation.kind
        })
        .collect();
    let tools = tools_for_installation(snapshot, installation);
    if games.is_empty() {
        group.add(
            &libadwaita::ActionRow::builder()
                .title(t("compat.no_games"))
                .subtitle(t("compat.no_games_desc"))
                .build(),
        );
        return group;
    }
    for game in games {
        group.add(&game_row(bridge, installation, game, &tools, content));
        if game.launcher != LauncherId::Steam {
            group.add(&dxvk_row(bridge, installation, game, content));
        }
    }
    group
}

fn game_row(
    bridge: &UiBridge,
    installation: &LauncherInstallation,
    game: &Game,
    tools: &[&CompatibilityTool],
    content: &GtkBox,
) -> libadwaita::ComboRow {
    let row = libadwaita::ComboRow::builder()
        .title(&game.name)
        .subtitle(t_args("compat.game_id", &[("id", &game.id)]))
        .build();
    let mut choices: Vec<Option<CompatibilityTool>> = vec![None];
    let mut labels = vec![t("compat.launcher_default")];
    for tool in tools {
        choices.push(Some((*tool).clone()));
        labels.push(tool.name.clone());
    }
    if let Some(selected) = &game.selected_tool {
        if !choices.iter().flatten().any(|tool| tool.id == *selected) {
            labels.push(selected.clone());
            choices.push(Some(CompatibilityTool {
                id: selected.clone(),
                name: selected.clone(),
                version: String::new(),
                path: PathBuf::new(),
                launcher: game.launcher,
                kind: ToolKind::Wine,
            }));
        }
    }
    let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    row.set_model(Some(&gtk4::StringList::new(&label_refs)));
    let selected = choices
        .iter()
        .position(|choice| {
            choice.as_ref().map(|tool| tool.id.as_str()) == game.selected_tool.as_deref()
        })
        .unwrap_or(0);
    row.set_selected(selected as u32);
    row.set_sensitive(game.writable);

    let choices = Rc::new(choices);
    let installation = installation.clone();
    let game = game.clone();
    let bridge = bridge.clone();
    let content = content.clone();
    row.connect_selected_notify(move |row| {
        let Some(tool) = choices.get(row.selected() as usize).cloned() else {
            return;
        };
        row.set_sensitive(false);
        let installation = installation.clone();
        let game = game.clone();
        let bridge_done = bridge.clone();
        let content_done = content.clone();
        run_action(
            move || match game.launcher {
                LauncherId::Steam => {
                    steam::set_game_tool(
                        &installation,
                        &game.id,
                        tool.as_ref().map(|tool| tool.id.as_str()),
                    )?;
                    Ok(())
                }
                LauncherId::Lutris => lutris::set_game_tool(
                    &game,
                    &installation,
                    tool.as_ref().map(|tool| tool.id.as_str()),
                ),
                LauncherId::Heroic => heroic::set_game_tool(&game, &installation, tool.as_ref()),
            },
            move |result| {
                match result {
                    Ok(()) => bridge_done.toast_msg(&t("compat.game_updated")),
                    Err(error) => bridge_done.toast_msg(&t_args(
                        "compat.action_failed",
                        &[("error", &error.to_string())],
                    )),
                }
                load_snapshot(&content_done, &bridge_done);
            },
        );
    });
    row
}

fn dxvk_row(
    bridge: &UiBridge,
    installation: &LauncherInstallation,
    game: &Game,
    content: &GtkBox,
) -> libadwaita::SwitchRow {
    let active =
        game.dxvk_enabled.unwrap_or(false) || game.prefix.as_deref().is_some_and(dxvk::is_managed);
    let row = libadwaita::SwitchRow::builder()
        .title(t_args("compat.dxvk_for", &[("name", &game.name)]))
        .subtitle(
            game.prefix
                .as_deref()
                .map(display_path)
                .unwrap_or_else(|| t("compat.prefix_missing")),
        )
        .active(active)
        .build();
    row.set_sensitive(game.writable && game.prefix.as_deref().is_some_and(Path::exists));

    let installation = installation.clone();
    let game = game.clone();
    let bridge = bridge.clone();
    let content = content.clone();
    row.connect_active_notify(move |row| {
        row.set_sensitive(false);
        let enabled = row.is_active();
        let installation = installation.clone();
        let game = game.clone();
        let runtime = bridge.store.runtime();
        let allow_prerelease = bridge.store.config().compatibility.release_channel == "prerelease";
        let bridge_done = bridge.clone();
        let content_done = content.clone();
        bridge.set_activity(Some(&t("compat.dxvk_working")));
        run_action(
            move || {
                let prefix = game
                    .prefix
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("game has no Wine prefix"))?;
                if enabled {
                    let release = runtime.block_on(async {
                        dxvk_releases_channel(1, allow_prerelease)
                            .await?
                            .into_iter()
                            .next()
                            .ok_or_else(|| anyhow::anyhow!("no DXVK release available"))
                    })?;
                    let artifact =
                        runtime.block_on(async { install_dxvk_release(&release).await })?;
                    dxvk::install(prefix, &artifact, &release.tag_name)?;
                } else if dxvk::is_managed(prefix) {
                    dxvk::rollback(prefix)?;
                }
                let config_result = match game.launcher {
                    LauncherId::Lutris => lutris::set_dxvk_enabled(&game, &installation, enabled),
                    LauncherId::Heroic => heroic::set_dxvk_enabled(&game, &installation, enabled),
                    LauncherId::Steam => Ok(()),
                };
                if config_result.is_err() && enabled {
                    let _ = dxvk::rollback(prefix);
                }
                config_result
            },
            move |result| {
                bridge_done.set_activity(None);
                match result {
                    Ok(()) => bridge_done.toast_msg(&t("compat.game_updated")),
                    Err(error) => bridge_done.toast_msg(&t_args(
                        "compat.action_failed",
                        &[("error", &error.to_string())],
                    )),
                }
                load_snapshot(&content_done, &bridge_done);
            },
        );
    });
    row
}

fn tools_for_installation<'a>(
    snapshot: &'a CompatibilitySnapshot,
    installation: &LauncherInstallation,
) -> Vec<&'a CompatibilityTool> {
    snapshot
        .tools
        .iter()
        .filter(|tool| {
            tool.launcher == installation.launcher
                && match installation.launcher {
                    LauncherId::Steam => tool.path.starts_with(&installation.root),
                    LauncherId::Lutris | LauncherId::Heroic => {
                        tool.path.starts_with(&installation.tool_root)
                    }
                }
        })
        .collect()
}

fn run_action<T, F, C>(operation: F, on_done: C)
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    C: FnOnce(anyhow::Result<T>) + 'static,
{
    let (tx, rx) = mpsc::channel();
    if std::thread::Builder::new()
        .name("tcms-compat-action".into())
        .spawn(move || {
            let _ = tx.send(operation());
        })
        .is_err()
    {
        on_done(Err(anyhow::anyhow!("could not start background worker")));
        return;
    }
    poll_local(rx, on_done);
}

fn poll_local<T: 'static>(rx: mpsc::Receiver<T>, on_done: impl FnOnce(T) + 'static) {
    let mut on_done = Some(on_done);
    glib::timeout_add_local(Duration::from_millis(50), move || match rx.try_recv() {
        Ok(value) => {
            if let Some(callback) = on_done.take() {
                callback(value);
            }
            glib::ControlFlow::Break
        }
        Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
    });
}

fn clear(container: &GtkBox) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn render_error(content: &GtkBox, message: &str) {
    clear(content);
    content.append(
        &libadwaita::StatusPage::builder()
            .icon_name("dialog-error-symbolic")
            .title(t("compat.error"))
            .description(message)
            .vexpand(true)
            .build(),
    );
}
