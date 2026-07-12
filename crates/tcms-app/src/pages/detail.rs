use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Label, Orientation, PolicyType, ScrolledWindow, TextView, WrapMode,
};
use libadwaita::prelude::*;
use tcms_core::i18n::{t, t_args};
use tcms_core::Package;

use crate::store::UiBridge;
use crate::widgets::{load_package_icon, package_action_button};

pub fn push_detail_page(nav: &libadwaita::NavigationView, bridge: &UiBridge, package: Package) {
    let loading = libadwaita::StatusPage::builder()
        .icon_name("content-loading-symbolic")
        .title(t("detail.loading"))
        .vexpand(true)
        .build();
    let page = libadwaita::NavigationPage::builder()
        .title(&package.name)
        .child(&loading)
        .build();
    nav.push(&page);

    let nav = nav.clone();
    let bridge = bridge.clone();
    let page_weak = page.downgrade();
    let store = bridge.store.clone();
    store.package_details_async(package, move |detailed, alts| {
        let Some(page) = page_weak.upgrade() else {
            return;
        };
        page.set_title(&detailed.name);
        page.set_child(Some(&build_detail_content(&detailed, &alts, &bridge, &nav)));
    });
}

fn build_detail_content(
    pkg: &Package,
    alts: &[Package],
    bridge: &UiBridge,
    _nav: &libadwaita::NavigationView,
) -> ScrolledWindow {
    let root = GtkBox::new(Orientation::Vertical, 18);
    root.set_margin_top(18);
    root.set_margin_bottom(24);
    root.set_margin_start(18);
    root.set_margin_end(18);

    let header = GtkBox::new(Orientation::Horizontal, 18);
    let icon = load_package_icon(pkg, bridge, 96);
    header.append(&icon);

    let titles = GtkBox::new(Orientation::Vertical, 6);
    titles.set_hexpand(true);
    titles.append(
        &Label::builder()
            .label(&pkg.name)
            .halign(Align::Start)
            .css_classes(["title-1"])
            .wrap(true)
            .build(),
    );
    titles.append(
        &Label::builder()
            .label(&pkg.summary)
            .halign(Align::Start)
            .css_classes(["dim-label"])
            .wrap(true)
            .build(),
    );
    let version = if pkg.version.is_empty() {
        String::new()
    } else {
        format!("{} · {}", t(pkg.id.source.i18n_key()), pkg.version)
    };
    if !version.is_empty() {
        titles.append(
            &Label::builder()
                .label(version)
                .halign(Align::Start)
                .css_classes(["caption"])
                .build(),
        );
    }
    header.append(&titles);

    let actions = GtkBox::new(Orientation::Vertical, 8);
    actions.set_valign(Align::Center);
    let action_btn = package_action_button(pkg, bridge);
    actions.append(&action_btn);
    if matches!(
        pkg.state,
        tcms_core::InstallState::Installed | tcms_core::InstallState::Updatable
    ) {
        let open_btn = gtk4::Button::builder()
            .label(t("action.open"))
            .css_classes(["pill"])
            .build();
        let pkg_open = pkg.clone();
        let win = bridge.window.clone();
        open_btn.connect_clicked(move |_| {
            if !crate::store::launch_package(&pkg_open, &win) {
                // Toast via banner title briefly is awkward; UriLauncher already tried.
            }
        });
        actions.append(&open_btn);
    }
    header.append(&actions);
    root.append(&header);

    if !pkg.description.is_empty() {
        root.append(&section_title(&t("detail.description")));
        root.append(
            &Label::builder()
                .label(&pkg.description)
                .halign(Align::Start)
                .wrap(true)
                .xalign(0.0)
                .build(),
        );
    }

    let meta = libadwaita::PreferencesGroup::builder()
        .title(t("detail.metadata"))
        .build();
    meta.add(&info_row(
        &t("detail.publisher"),
        pkg.display_publisher().unwrap_or(&t("detail.unknown")),
    ));
    meta.add(&info_row(
        &t("detail.license"),
        pkg.license.as_deref().unwrap_or(&t("detail.unknown")),
    ));
    let license_kind = match pkg.is_proprietary {
        Some(true) => t("detail.proprietary"),
        Some(false) => t("detail.opensource"),
        None => t("detail.unknown"),
    };
    meta.add(&info_row(&t("detail.license_kind"), &license_kind));
    if let Some(home) = pkg.homepage.as_deref() {
        meta.add(&info_row(&t("detail.homepage"), home));
    }
    if let Some(donate) = pkg.donate_url.as_deref() {
        meta.add(&info_row(&t("detail.donate"), donate));
    }
    if let Some(size) = pkg.size_bytes {
        meta.add(&info_row(&t("detail.size"), &format_size(size)));
    }
    root.append(&meta);

    let sources = libadwaita::PreferencesGroup::builder()
        .title(t("detail.sources"))
        .description(t("detail.sources_desc"))
        .build();
    sources.add(&source_row(pkg, bridge));
    for alt in alts {
        sources.add(&source_row(alt, bridge));
    }
    root.append(&sources);

    let perms = libadwaita::PreferencesGroup::builder()
        .title(t("detail.permissions"))
        .build();
    let perm_text = pkg
        .permissions
        .clone()
        .unwrap_or_else(|| t("detail.permissions_unknown"));
    perms.add(
        &libadwaita::ActionRow::builder()
            .title(t("detail.permissions"))
            .subtitle(perm_text)
            .build(),
    );
    root.append(&perms);

    let copy_id = gtk4::Button::builder()
        .label(t("detail.copy_id"))
        .icon_name("edit-copy-symbolic")
        .halign(Align::Start)
        .css_classes(["flat"])
        .build();
    let id_text = pkg.id.to_string();
    let bridge_c = bridge.clone();
    copy_id.connect_clicked(move |_| {
        if let Some(display) = gtk4::gdk::Display::default() {
            display.clipboard().set_text(&id_text);
            bridge_c.toast_msg(&t("detail.id_copied"));
        }
    });
    root.append(&copy_id);

    let report = gtk4::Button::builder()
        .label(t("detail.report_bug"))
        .icon_name("dialog-warning-symbolic")
        .halign(Align::Start)
        .css_classes(["flat"])
        .build();
    let bridge_r = bridge.clone();
    let pkg_r = pkg.clone();
    report.connect_clicked(move |_| {
        open_bug_report(&bridge_r, &pkg_r);
    });
    root.append(&report);

    ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vexpand(true)
        .child(&root)
        .build()
}

fn section_title(text: &str) -> Label {
    Label::builder()
        .label(text)
        .halign(Align::Start)
        .css_classes(["title-2"])
        .build()
}

fn info_row(title: &str, subtitle: &str) -> libadwaita::ActionRow {
    libadwaita::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .build()
}

fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

fn source_row(pkg: &Package, bridge: &UiBridge) -> libadwaita::ActionRow {
    let row = libadwaita::ActionRow::builder()
        .title(t(pkg.id.source.i18n_key()))
        .subtitle(&pkg.id.id)
        .build();
    let btn = package_action_button(pkg, bridge);
    row.add_suffix(&btn);
    row
}

fn open_bug_report(bridge: &UiBridge, pkg: &Package) {
    let dialog = libadwaita::AlertDialog::builder()
        .heading(t("detail.report_bug"))
        .body(t_args("detail.report_bug_body", &[("name", &pkg.name)]))
        .build();

    let box_ = GtkBox::new(Orientation::Vertical, 8);
    let hint = Label::builder()
        .label(t("detail.report_bug_hint"))
        .wrap(true)
        .xalign(0.0)
        .build();
    let text = TextView::builder()
        .wrap_mode(WrapMode::WordChar)
        .accepts_tab(false)
        .build();
    text.set_size_request(-1, 120);
    let frame = gtk4::Frame::builder().child(&text).build();
    box_.append(&hint);
    box_.append(&frame);
    dialog.set_extra_child(Some(&box_));
    dialog.add_response("cancel", &t("action.cancel"));
    dialog.add_response("send", &t("detail.report_send"));
    dialog.set_response_appearance("send", libadwaita::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("send"));
    dialog.set_close_response("cancel");

    let bridge = bridge.clone();
    let pkg = pkg.clone();
    let text_buf = text.buffer();
    let window = bridge.window.clone();
    dialog.connect_response(None, move |_, response| {
        if response != "send" {
            return;
        }
        let start = text_buf.start_iter();
        let end = text_buf.end_iter();
        let body = text_buf.text(&start, &end, false).trim().to_string();
        if body.is_empty() {
            bridge.toast_msg(&t("detail.report_empty"));
            return;
        }
        deliver_bug_report(&bridge, &pkg, &body);
    });
    dialog.present(Some(&window));
}

fn deliver_bug_report(bridge: &UiBridge, pkg: &Package, user_report: &str) {
    let subject = format!("[Bug] {} ({})", pkg.name, pkg.id);
    let body = format!(
        "Package: {}\nSource: {}\nVersion: {}\n\nReport:\n{}\n",
        pkg.id,
        pkg.id.source.as_str(),
        pkg.version,
        user_report
    );

    // Prefer mailto when publisher looks like an email.
    if let Some(dev) = pkg.display_publisher() {
        if dev.contains('@') && !dev.contains('<') {
            let uri = format!(
                "mailto:{}?subject={}&body={}",
                url_encode(dev),
                url_encode(&subject),
                url_encode(&body)
            );
            if launch_uri(&bridge.window, &uri) {
                bridge.toast_msg(&t("detail.report_sent"));
                return;
            }
        }
        // "Name <email>"
        if let Some(start) = dev.find('<') {
            if let Some(end) = dev.find('>') {
                let email = &dev[start + 1..end];
                if email.contains('@') {
                    let uri = format!(
                        "mailto:{}?subject={}&body={}",
                        url_encode(email),
                        url_encode(&subject),
                        url_encode(&body)
                    );
                    if launch_uri(&bridge.window, &uri) {
                        bridge.toast_msg(&t("detail.report_sent"));
                        return;
                    }
                }
            }
        }
    }

    if let Some(bug) = pkg.bug_url.as_deref().filter(|u| !u.is_empty()) {
        // Open tracker and copy the report for the user.
        let display = gtk4::gdk::Display::default();
        if let Some(display) = display {
            display.clipboard().set_text(&body);
        }
        if launch_uri(&bridge.window, bug) {
            bridge.toast_msg(&t("detail.report_copied_open"));
            return;
        }
    }

    if let Some(home) = pkg.homepage.as_deref() {
        let display = gtk4::gdk::Display::default();
        if let Some(display) = display {
            display.clipboard().set_text(&body);
        }
        if launch_uri(&bridge.window, home) {
            bridge.toast_msg(&t("detail.report_copied_open"));
            return;
        }
    }

    let display = gtk4::gdk::Display::default();
    if let Some(display) = display {
        display.clipboard().set_text(&body);
        bridge.toast_msg(&t("detail.report_copied"));
    } else {
        bridge.toast_msg(&t("detail.report_failed"));
    }
}

fn launch_uri(window: &impl IsA<gtk4::Window>, uri: &str) -> bool {
    let launcher = gtk4::UriLauncher::new(uri);
    launcher.launch(Some(window), gio::Cancellable::NONE, |_| {});
    true
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
