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
    let toolbar = libadwaita::ToolbarView::new();
    let header = libadwaita::HeaderBar::new();
    toolbar.add_top_bar(&header);

    let loading = libadwaita::StatusPage::builder()
        .icon_name("content-loading-symbolic")
        .title(t("detail.loading"))
        .vexpand(true)
        .build();
    toolbar.set_content(Some(&loading));

    let page = libadwaita::NavigationPage::builder()
        .title(&package.name)
        .child(&toolbar)
        .build();
    nav.push(&page);

    let bridge = bridge.clone();
    let page_weak = page.downgrade();
    let toolbar_weak = toolbar.downgrade();
    let store = bridge.store.clone();
    store.package_details_async(package, move |detailed, alts| {
        let Some(page) = page_weak.upgrade() else {
            return;
        };
        let Some(toolbar) = toolbar_weak.upgrade() else {
            return;
        };
        page.set_title(&detailed.name);
        toolbar.set_content(Some(&build_detail_content(&detailed, &alts, &bridge)));
    });
}

fn build_detail_content(pkg: &Package, alts: &[Package], bridge: &UiBridge) -> ScrolledWindow {
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
            .wrap_mode(gtk4::pango::WrapMode::WordChar)
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
        format!("{} · {}", t(pkg.source_i18n_key()), pkg.version)
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

    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.set_valign(Align::Center);
    // Install belongs only next to each repository below. The header shows
    // update/remove/open for a source that is actually installed on this system.
    if let Some(installed) = header_action_package(pkg, alts) {
        if installed.state == tcms_core::InstallState::Updatable {
            let update_btn = package_action_button(installed, bridge);
            actions.append(&update_btn);
            let remove_btn = gtk4::Button::builder()
                .label(t("action.remove"))
                .css_classes(["pill", "destructive-action"])
                .build();
            let bridge_rm = bridge.clone();
            let pkg_rm = installed.clone();
            remove_btn.connect_clicked(move |btn| {
                btn.set_sensitive(false);
                bridge_rm.run_action(tcms_core::PackageAction::Remove, &pkg_rm, btn);
            });
            actions.append(&remove_btn);
        } else {
            let remove_btn = package_action_button(installed, bridge);
            actions.append(&remove_btn);
        }
        let open_btn = gtk4::Button::builder()
            .visible(
                installed.desktop_id.is_some()
                    || installed
                        .id
                        .flatpak
                        .as_ref()
                        .is_some_and(|reference| reference.kind == "app"),
            )
            .label(t("action.open"))
            .css_classes(["pill"])
            .build();
        let pkg_open = installed.clone();
        let bridge_open = bridge.clone();
        open_btn.connect_clicked(move |_| {
            bridge_open.launch_installed(&pkg_open);
        });
        actions.append(&open_btn);
    }
    root.append(&header);
    root.append(&actions);

    if !pkg.screenshots.is_empty() {
        root.append(&section_title(&t("detail.screenshots")));
        root.append(&screenshot_gallery(pkg, bridge));
    }
    if !pkg.description.is_empty() {
        root.append(&section_title(&t("detail.description")));
        root.append(
            &Label::builder()
                .label(&pkg.description)
                .wrap_mode(gtk4::pango::WrapMode::WordChar)
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
        None,
        bridge,
    ));
    meta.add(&info_row(
        &t("detail.license"),
        pkg.license.as_deref().unwrap_or(&t("detail.unknown")),
        None,
        bridge,
    ));
    let license_kind = match pkg.is_proprietary {
        Some(true) => t("detail.proprietary"),
        Some(false) => t("detail.opensource"),
        None => t("detail.unknown"),
    };
    meta.add(&info_row(
        &t("detail.license_kind"),
        &license_kind,
        None,
        bridge,
    ));
    if let Some(home) = pkg.homepage.as_deref() {
        meta.add(&info_row(&t("detail.homepage"), home, Some(home), bridge));
    }
    if let Some(donate) = pkg.donate_url.as_deref() {
        meta.add(&info_row(&t("detail.donate"), donate, Some(donate), bridge));
    }
    if let Some(size) = pkg.size_bytes {
        meta.add(&info_row(
            &t("detail.size"),
            &format_size(size),
            None,
            bridge,
        ));
    }
    if !pkg.categories.is_empty() {
        meta.add(&info_row(
            &t("detail.categories"),
            &pkg.categories.join(" · "),
            None,
            bridge,
        ));
    }
    root.append(&meta);
    if !pkg.releases.is_empty() {
        let releases = libadwaita::PreferencesGroup::builder()
            .title(t("detail.releases"))
            .build();
        for release in &pkg.releases {
            let title = match &release.date {
                Some(date) => format!("{} · {date}", release.version),
                None => release.version.clone(),
            };
            let description = if release.description.is_empty() {
                t("detail.no_release_notes")
            } else {
                release.description.clone()
            };
            releases.add(&info_row(
                &title,
                &description,
                release.url.as_deref(),
                bridge,
            ));
        }
        root.append(&releases);
    }

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
            .use_markup(false)
            .title(t("detail.permissions"))
            .subtitle(perm_text)
            .build(),
    );
    if pkg.id.flatpak.as_ref().is_some_and(|r| r.kind == "app")
        && !pkg.installed_elsewhere
        && matches!(
            pkg.state,
            tcms_core::InstallState::Installed | tcms_core::InstallState::Updatable
        )
    {
        let edit = gtk4::Button::with_label(&t("permissions.title"));
        let bridge = bridge.clone();
        let id = pkg.id.clone();
        edit.connect_clicked(move |_| crate::management::permissions(&bridge, id.clone()));
        perms.add(&edit);
    }
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

fn screenshot_gallery(pkg: &Package, bridge: &UiBridge) -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 8);
    let carousel = libadwaita::Carousel::builder()
        .hexpand(true)
        .allow_mouse_drag(true)
        .build();
    for screenshot in pkg.screenshots.iter().take(5) {
        let page = GtkBox::new(Orientation::Vertical, 6);
        let picture = gtk4::Picture::builder()
            .content_fit(gtk4::ContentFit::Contain)
            .can_shrink(true)
            .height_request(280)
            .hexpand(true)
            .build();
        let caption = screenshot
            .caption
            .clone()
            .unwrap_or_else(|| pkg.name.clone());
        picture.set_alternative_text(Some(&caption));
        let status = Label::builder()
            .label(t("detail.loading_image"))
            .wrap(true)
            .build();
        page.append(&picture);
        page.append(&status);
        carousel.append(&page);
        let (tx, rx) = std::sync::mpsc::channel();
        let url = screenshot.url.clone();
        bridge.store.runtime().spawn(async move {
            let _ = tx.send(tcms_core::screenshots::cached(&url).await);
        });
        let weak_picture = picture.downgrade();
        let weak_status = status.downgrade();
        glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            let (Some(picture), Some(status)) = (weak_picture.upgrade(), weak_status.upgrade())
            else {
                return glib::ControlFlow::Break;
            };
            match rx.try_recv() {
                Ok(Ok(path)) => {
                    match gtk4::gdk::Texture::from_filename(path) {
                        Ok(texture) => {
                            picture.set_paintable(Some(&texture));
                            status.set_label(&caption);
                        }
                        Err(_) => status.set_label(&t("detail.image_failed")),
                    }
                    glib::ControlFlow::Break
                }
                Ok(Err(_)) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    status.set_label(&t("detail.image_failed"));
                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            }
        });
    }
    root.append(&carousel);
    let controls = GtkBox::new(Orientation::Horizontal, 8);
    controls.set_halign(Align::Center);
    for (icon, label, direction) in [
        ("go-previous-symbolic", "detail.previous_image", -1.0),
        ("go-next-symbolic", "detail.next_image", 1.0),
    ] {
        let button = gtk4::Button::builder()
            .icon_name(icon)
            .tooltip_text(t(label))
            .build();
        let carousel = carousel.clone();
        button.connect_clicked(move |_| {
            let next = (carousel.position().round() + direction)
                .clamp(0.0, f64::from(carousel.n_pages().saturating_sub(1)))
                as u32;
            carousel.scroll_to(&carousel.nth_page(next), true);
        });
        controls.append(&button);
    }
    root.append(&controls);
    root
}

fn section_title(text: &str) -> Label {
    Label::builder()
        .label(text)
        .halign(Align::Start)
        .css_classes(["title-2"])
        .build()
}

fn header_action_package<'a>(pkg: &'a Package, alts: &'a [Package]) -> Option<&'a Package> {
    std::iter::once(pkg).chain(alts.iter()).find(|candidate| {
        !candidate.installed_elsewhere
            && matches!(
                candidate.state,
                tcms_core::InstallState::Installed | tcms_core::InstallState::Updatable
            )
    })
}

fn info_row(
    title: &str,
    subtitle: &str,
    link: Option<&str>,
    bridge: &UiBridge,
) -> libadwaita::ActionRow {
    let row = libadwaita::ActionRow::builder()
        .use_markup(false)
        .title(title)
        .subtitle(subtitle)
        .build();
    if let Some(uri) = link.filter(|u| u.starts_with("http://") || u.starts_with("https://")) {
        row.set_activatable(true);
        let window = bridge.window.clone();
        let uri = uri.to_string();
        row.connect_activated(move |_| {
            let _ = launch_uri(&window, &uri);
        });
        row.add_suffix(&gtk4::Image::from_icon_name("adw-external-link-symbolic"));
    }
    row
}

fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GiB", b / GB)
    } else if b >= MB {
        format!("{:.1} MiB", b / MB)
    } else if b >= KB {
        format!("{:.0} KiB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

fn source_row(pkg: &Package, bridge: &UiBridge) -> libadwaita::ActionRow {
    let state = if pkg.installed_elsewhere {
        t("state.available")
    } else {
        state_label_local(pkg.state)
    };
    let row = libadwaita::ActionRow::builder()
        .use_markup(false)
        .title(t(pkg.source_i18n_key()))
        .subtitle(format!(
            "{} · {}\n{} · {}",
            pkg.id.display_ref(),
            state,
            if pkg.version.is_empty() {
                t("detail.unknown")
            } else if let Some(next) = &pkg.available_version {
                format!("{} → {next}", pkg.version)
            } else {
                pkg.version.clone()
            },
            pkg.size_bytes
                .map(format_size)
                .unwrap_or_else(|| t("inventory.size_unknown"))
        ))
        .build();
    let btn = package_action_button(pkg, bridge);
    row.add_suffix(&btn);
    row
}

fn state_label_local(state: tcms_core::InstallState) -> String {
    match state {
        tcms_core::InstallState::Available => t("state.available"),
        tcms_core::InstallState::Installed => t("state.installed"),
        tcms_core::InstallState::Updatable => t("state.updatable"),
        tcms_core::InstallState::Installing => t("state.installing"),
        tcms_core::InstallState::Removing => t("state.removing"),
    }
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
