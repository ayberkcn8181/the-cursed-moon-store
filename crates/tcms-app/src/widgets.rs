use std::path::Path;

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, ListBox, ListBoxRow, Orientation, PolicyType, ScrolledWindow};
use libadwaita::prelude::*;
use tcms_core::i18n::t;
use tcms_core::icons::is_remote_icon;
use tcms_core::{InstallState, Package, PackageAction};

use crate::store::UiBridge;

pub fn package_list(packages: &[Package], bridge: &UiBridge) -> ScrolledWindow {
    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .build();
    if packages.is_empty() {
        scroll.set_child(Some(
            &libadwaita::StatusPage::builder()
                .icon_name("edit-find-symbolic")
                .title(t("package.none_title"))
                .description(t("package.none_desc"))
                .build(),
        ));
        return scroll;
    }
    let model = gio::ListStore::new::<glib::BoxedAnyObject>();
    let items: Vec<_> = packages
        .iter()
        .cloned()
        .map(glib::BoxedAnyObject::new)
        .collect();
    model.splice(0, 0, &items);
    let factory = gtk4::SignalListItemFactory::new();
    let bridge = bridge.clone();
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk4::ListItem>().unwrap();
        let object = item
            .item()
            .unwrap()
            .downcast::<glib::BoxedAnyObject>()
            .unwrap();
        let package = object.borrow::<Package>();
        let host = ListBox::builder()
            .selection_mode(gtk4::SelectionMode::None)
            .build();
        host.append(&package_row(&package, &bridge));
        item.set_child(Some(&host));
    });
    factory.connect_unbind(|_, item| {
        item.downcast_ref::<gtk4::ListItem>()
            .unwrap()
            .set_child(None::<&gtk4::Widget>)
    });
    let list = gtk4::ListView::new(Some(gtk4::NoSelection::new(Some(model))), Some(factory));
    list.add_css_class("boxed-list");
    scroll.set_child(Some(&list));
    scroll
}

pub fn paged_package_list(packages: &[Package], bridge: &UiBridge) -> GtkBox {
    use std::{cell::Cell, rc::Rc};
    use tcms_core::{catalog, PackageSource};
    let root = GtkBox::new(Orientation::Vertical, 8);
    let controls = GtkBox::new(Orientation::Horizontal, 8);
    let source = gtk4::DropDown::from_strings(&[
        &t("catalog.all_sources"),
        &t("source.pacman"),
        &t("source.flatpak"),
        &t("source.aur"),
    ]);
    let previous = gtk4::Button::with_label(&t("catalog.previous"));
    let next = gtk4::Button::with_label(&t("catalog.next"));
    let summary = Label::builder().hexpand(true).build();
    controls.append(&source);
    controls.append(&previous);
    controls.append(&summary);
    controls.append(&next);
    let host = GtkBox::new(Orientation::Vertical, 0);
    host.set_vexpand(true);
    root.append(&controls);
    root.append(&host);
    let packages = packages.to_vec();
    let page = Rc::new(Cell::new(0usize));
    let selected = Rc::new(Cell::new(None));
    let render: Rc<dyn Fn()> = {
        let page = page.clone();
        let selected = selected.clone();
        let host = host.downgrade();
        let previous = previous.downgrade();
        let next = next.downgrade();
        let summary = summary.downgrade();
        let bridge = bridge.clone();
        Rc::new(move || {
            let (Some(host), Some(previous), Some(next), Some(summary)) = (
                host.upgrade(),
                previous.upgrade(),
                next.upgrade(),
                summary.upgrade(),
            ) else {
                return;
            };
            let (visible, current, total) = catalog::page(&packages, selected.get(), page.get());
            page.set(current);
            previous.set_sensitive(current > 0);
            next.set_sensitive((current + 1) * catalog::PAGE_SIZE < total);
            summary.set_text(&format!(
                "{}–{} / {}",
                if total == 0 {
                    0
                } else {
                    current * catalog::PAGE_SIZE + 1
                },
                current * catalog::PAGE_SIZE + visible.len(),
                total
            ));
            while let Some(child) = host.first_child() {
                host.remove(&child);
            }
            host.append(&package_list(&visible, &bridge));
        })
    };
    {
        let render = render.clone();
        let page = page.clone();
        previous.connect_clicked(move |_| {
            page.set(page.get().saturating_sub(1));
            render();
        });
    }
    {
        let render = render.clone();
        let page = page.clone();
        next.connect_clicked(move |_| {
            page.set(page.get() + 1);
            render();
        });
    }
    {
        let render = render.clone();
        source.connect_selected_notify(move |d| {
            selected.set(match d.selected() {
                1 => Some(PackageSource::Pacman),
                2 => Some(PackageSource::Flatpak),
                3 => Some(PackageSource::Aur),
                _ => None,
            });
            page.set(0);
            render();
        });
    }
    render();
    root
}

pub fn installed_package_list(packages: &[Package], bridge: &UiBridge) -> GtkBox {
    let content = GtkBox::new(Orientation::Vertical, 8);
    let search = gtk4::SearchEntry::builder()
        .placeholder_text(t("installed.search"))
        .hexpand(true)
        .build();
    content.append(&search);
    let model = gio::ListStore::new::<glib::BoxedAnyObject>();
    for package in packages {
        model.append(&glib::BoxedAnyObject::new(package.clone()));
    }
    let query = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
    let query_filter = query.clone();
    let filter = gtk4::CustomFilter::new(move |object| {
        let package = object
            .downcast_ref::<glib::BoxedAnyObject>()
            .unwrap()
            .borrow::<Package>();
        let query = query_filter.borrow();
        package.name.to_lowercase().contains(query.as_str())
            || package.id.id.to_lowercase().contains(query.as_str())
            || package.summary.to_lowercase().contains(query.as_str())
    });
    let filtered = gtk4::FilterListModel::new(Some(model), Some(filter.clone()));
    search.connect_search_changed(move |search| {
        *query.borrow_mut() = search.text().trim().to_lowercase();
        filter.changed(gtk4::FilterChange::Different);
    });
    let factory = gtk4::SignalListItemFactory::new();
    let bridge = bridge.clone();
    factory.connect_bind(move |_, object| {
        let item = object.downcast_ref::<gtk4::ListItem>().unwrap();
        let value = item
            .item()
            .unwrap()
            .downcast::<glib::BoxedAnyObject>()
            .unwrap();
        // An ActionRow needs a ListBox parent for its activation and keyboard
        // behavior. This single-row box lives only while the item is bound.
        let row_host = ListBox::builder()
            .selection_mode(gtk4::SelectionMode::None)
            .build();
        row_host.append(&package_row(&value.borrow::<Package>(), &bridge));
        item.set_activatable(false);
        item.set_child(Some(&row_host));
    });
    factory.connect_unbind(|_, object| {
        object
            .downcast_ref::<gtk4::ListItem>()
            .unwrap()
            .set_child(gtk4::Widget::NONE);
    });
    let list = gtk4::ListView::new(Some(gtk4::NoSelection::new(Some(filtered))), Some(factory));
    list.add_css_class("boxed-list");
    content.append(
        &ScrolledWindow::builder()
            .hscrollbar_policy(PolicyType::Never)
            .vexpand(true)
            .hexpand(true)
            .child(&list)
            .build(),
    );
    content.set_vexpand(true);
    content
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
        bridge_btn.run_action(action, &pkg_btn, btn);
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

/// Keep partial results visible while explaining which source was unavailable.
pub fn append_listing_errors(host: &GtkBox, errors: &[String]) {
    if errors.is_empty() {
        return;
    }
    let label = Label::builder()
        .label(format!(
            "{}\n{}",
            tcms_core::i18n::t("catalog.load_failed"),
            errors.join("\n")
        ))
        .wrap(true)
        .xalign(0.0)
        .selectable(true)
        .css_classes(["error"])
        .build();
    host.append(&label);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    fn descendants<T: IsA<gtk4::Widget> + glib::types::StaticType>(root: &gtk4::Widget) -> Vec<T> {
        let mut result = Vec::new();
        let mut child = root.first_child();
        while let Some(widget) = child {
            if let Ok(value) = widget.clone().downcast::<T>() {
                result.push(value);
            }
            result.extend(descendants::<T>(&widget));
            child = widget.next_sibling();
        }
        result
    }

    fn spin_until(predicate: impl Fn() -> bool) {
        let context = glib::MainContext::default();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !predicate() {
            assert!(Instant::now() < deadline, "GTK did not settle");
            for _ in 0..100 {
                if !context.pending() {
                    break;
                }
                context.iteration(false);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    #[ignore = "requires a display; CI runs this under Xvfb"]
    fn installed_inventory_filters_virtualizes_and_opens_correct_package() {
        libadwaita::init().unwrap();
        let store = crate::store::StoreService::from_config(tcms_core::AppConfig::default());
        let window = gtk4::Window::builder()
            .default_width(800)
            .default_height(600)
            .build();
        let opened = Rc::new(std::cell::RefCell::new(None));
        let opened_callback = opened.clone();
        let bridge = UiBridge {
            icons: crate::icon_loader::IconLoader::new(store.runtime()),
            store,
            toast: libadwaita::ToastOverlay::new(),
            reload: Rc::new(|| {}),
            open_detail: Rc::new(move |package| {
                *opened_callback.borrow_mut() = Some(package.id.id)
            }),
            window: window.clone(),
            busy: Default::default(),
            activity: libadwaita::Banner::new(""),
            transaction_log: gtk4::TextBuffer::new(None),
            pending_transactions: Default::default(),
        };
        let packages: Vec<_> = (0..2000)
            .map(|index| {
                let id = format!("package-{index:05}");
                Package::stub(
                    tcms_core::PackageSource::Pacman,
                    &id,
                    &id,
                    "",
                    "1",
                    InstallState::Installed,
                )
            })
            .collect();
        let content = installed_package_list(&packages, &bridge);
        let search = content
            .first_child()
            .unwrap()
            .downcast::<gtk4::SearchEntry>()
            .unwrap();
        window.set_child(Some(&content));
        window.present();
        spin_until(|| !descendants::<libadwaita::ActionRow>(content.upcast_ref()).is_empty());
        let list = descendants::<gtk4::ListView>(content.upcast_ref())
            .pop()
            .unwrap();
        assert_eq!(list.model().unwrap().n_items(), 2000);
        assert!(
            descendants::<libadwaita::ActionRow>(content.upcast_ref()).len() < 2000,
            "the full inventory must not allocate a widget for every package"
        );

        search.set_text("PACKAGE-01999");
        search.emit_by_name::<()>("search-changed", &[]);
        spin_until(|| list.model().unwrap().n_items() == 1);
        spin_until(|| {
            descendants::<libadwaita::ActionRow>(content.upcast_ref())
                .iter()
                .any(|row| row.title() == "package-01999")
        });
        let row = descendants::<libadwaita::ActionRow>(content.upcast_ref())
            .into_iter()
            .find(|row| row.title() == "package-01999")
            .unwrap();
        row.emit_by_name::<()>("activated", &[]);
        assert_eq!(opened.borrow().as_deref(), Some("package-01999"));
        search.set_text("does-not-exist");
        search.emit_by_name::<()>("search-changed", &[]);
        assert_eq!(list.model().unwrap().n_items(), 0);
        search.set_text("");
        search.emit_by_name::<()>("search-changed", &[]);
        assert_eq!(list.model().unwrap().n_items(), 2000);
        window.set_child(gtk4::Widget::NONE);
        window.close();
    }
}
