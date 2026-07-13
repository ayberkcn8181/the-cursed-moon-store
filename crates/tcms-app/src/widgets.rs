use std::path::Path;

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, ListBox, ListBoxRow, Orientation, PolicyType, ScrolledWindow};
use libadwaita::prelude::*;
use tcms_core::i18n::t;
use tcms_core::icons::is_remote_icon;
use tcms_core::{InstallState, Package, PackageAction};

use crate::store::UiBridge;

pub fn package_list(packages: &[Package], bridge: &UiBridge) -> ScrolledWindow {
    let list = ListBox::builder()
        .selection_mode(gtk4::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();

    if packages.is_empty() {
        let empty = libadwaita::StatusPage::builder()
            .icon_name("edit-find-symbolic")
            .title(t("package.none_title"))
            .description(t("package.none_desc"))
            .build();
        list.append(&empty);
    } else {
        for pkg in packages {
            list.append(&package_row(pkg, bridge));
        }
    }

    ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .child(&list)
        .build()
}

fn package_row(pkg: &Package, bridge: &UiBridge) -> ListBoxRow {
    let version_bit = match (&pkg.available_version, pkg.state) {
        (Some(avail), InstallState::Updatable) => format!("{} → {avail}", pkg.version),
        _ => pkg.version.clone(),
    };
    let source = t(pkg.id.source.i18n_key());
    let summary = if pkg.summary.chars().count() > 90 {
        let s: String = pkg.summary.chars().take(87).collect();
        format!("{s}…")
    } else {
        pkg.summary.clone()
    };
    let row = libadwaita::ActionRow::builder()
        .title(&pkg.name)
        .subtitle(format!("{summary}\n{source} · {version_bit}"))
        .activatable(true)
        .build();

    let icon = load_package_icon(pkg, bridge, 42);
    row.add_prefix(&icon);

    let badge = Label::builder()
        .label(state_label(pkg.state))
        .css_classes(["dim-label", "caption"])
        .build();
    row.add_suffix(&badge);

    if let Some(button) = list_action_button(pkg, bridge) {
        row.add_suffix(&button);
    }

    if pkg.state == InstallState::Updatable && !pkg.installed_elsewhere {
        let remove_btn = gtk4::Button::builder()
            .label(t("action.remove"))
            .valign(gtk4::Align::Center)
            .css_classes(["flat"])
            .build();
        let bridge_rm = bridge.clone();
        let pkg_rm = pkg.clone();
        remove_btn.connect_clicked(move |_| {
            bridge_rm.run_action(PackageAction::Remove, &pkg_rm);
        });
        row.add_suffix(&remove_btn);
    }

    let bridge_open = bridge.clone();
    let pkg_open = pkg.clone();
    row.connect_activated(move |_| {
        bridge_open.open_package(&pkg_open);
    });

    // ActionRow is already a ListBoxRow; nesting it inside another ListBoxRow
    // prevents the activated signal from firing when the list row is clicked.
    row.upcast()
}

/// Actions for Explore / Installed / Updates lists.
/// Install is intentionally omitted here — it only appears next to each
/// repository on the detail page.
pub fn list_action_button(pkg: &Package, bridge: &UiBridge) -> Option<gtk4::Button> {
    if pkg.installed_elsewhere {
        return None;
    }
    match pkg.state {
        InstallState::Available => None,
        InstallState::Installed => Some(action_button(
            PackageAction::Remove,
            t("action.remove"),
            true,
            pkg,
            bridge,
        )),
        InstallState::Updatable => Some(action_button(
            PackageAction::Update,
            t("action.update"),
            false,
            pkg,
            bridge,
        )),
        InstallState::Installing | InstallState::Removing => Some(action_button(
            PackageAction::Install,
            "…".to_string(),
            false,
            pkg,
            bridge,
        )),
    }
}

/// Per-repository action used on the detail page source rows.
pub fn package_action_button(pkg: &Package, bridge: &UiBridge) -> gtk4::Button {
    let source_state = if pkg.installed_elsewhere {
        InstallState::Available
    } else {
        pkg.state
    };
    let (action, label, destructive) = match source_state {
        InstallState::Available => (PackageAction::Install, t("action.install"), false),
        InstallState::Installed => (PackageAction::Remove, t("action.remove"), true),
        InstallState::Updatable => (PackageAction::Update, t("action.update"), false),
        InstallState::Installing | InstallState::Removing => {
            (PackageAction::Install, "…".to_string(), false)
        }
    };
    action_button(action, label, destructive, pkg, bridge)
}

fn action_button(
    action: PackageAction,
    label: String,
    destructive: bool,
    pkg: &Package,
    bridge: &UiBridge,
) -> gtk4::Button {
    let button = gtk4::Button::builder()
        .label(&label)
        .valign(gtk4::Align::Center)
        .build();
    if destructive {
        button.add_css_class("destructive-action");
    } else {
        button.add_css_class("suggested-action");
    }
    button.add_css_class("pill");

    if matches!(pkg.state, InstallState::Installing | InstallState::Removing) || label == "…" {
        button.set_sensitive(false);
    }

    let bridge_btn = bridge.clone();
    let pkg_btn = pkg.clone();
    button.connect_clicked(move |btn| {
        btn.set_sensitive(false);
        bridge_btn.run_action(action, &pkg_btn);
    });
    button
}

pub fn load_package_icon(pkg: &Package, bridge: &UiBridge, pixel_size: i32) -> gtk4::Image {
    let image = gtk4::Image::from_icon_name("application-x-executable");
    if let Some(name) = pkg.icon_name.as_deref() {
        if !is_remote_icon(name) && Path::new(name).exists() {
            image.set_from_file(Some(name));
            image.set_pixel_size(pixel_size);
            return image;
        }
    }
    bridge.icons.bind(pkg, &image, pixel_size);
    image
}

fn state_label(state: InstallState) -> String {
    match state {
        InstallState::Available => t("state.available"),
        InstallState::Installed => t("state.installed"),
        InstallState::Updatable => t("state.updatable"),
        InstallState::Installing => t("state.installing"),
        InstallState::Removing => t("state.removing"),
    }
}

pub fn page_shell(title: &str, child: &impl IsA<gtk4::Widget>) -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, 12);
    page.set_margin_top(18);
    page.set_margin_bottom(18);
    page.set_margin_start(18);
    page.set_margin_end(18);
    page.set_hexpand(true);
    page.set_vexpand(true);

    let heading = Label::builder()
        .label(title)
        .halign(gtk4::Align::Start)
        .css_classes(["title-1"])
        .build();
    page.append(&heading);
    page.append(child);
    page
}

pub fn featured_view(sections: &[tcms_core::FeaturedSection], bridge: &UiBridge) -> ScrolledWindow {
    let content = GtkBox::new(Orientation::Vertical, 18);
    content.set_hexpand(true);

    if sections.is_empty() {
        let empty = libadwaita::StatusPage::builder()
            .icon_name("emblem-favorite-symbolic")
            .title(t("featured.unavailable"))
            .description(t("featured.empty_desc"))
            .vexpand(true)
            .build();
        content.append(&empty);
    } else {
        for section in sections {
            let heading = Label::builder()
                .label(t(&section.title_key))
                .halign(gtk4::Align::Start)
                .css_classes(["title-2"])
                .build();
            content.append(&heading);

            let list = ListBox::builder()
                .selection_mode(gtk4::SelectionMode::None)
                .css_classes(["boxed-list"])
                .build();
            for pkg in section.packages.iter().take(6) {
                list.append(&package_row(pkg, bridge));
            }
            content.append(&list);
        }
    }

    ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .child(&content)
        .build()
}
