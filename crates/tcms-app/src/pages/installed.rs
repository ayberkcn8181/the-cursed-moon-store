use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Orientation, Spinner};

use tcms_core::i18n::{t, t_args};

use crate::store::{ListKind, UiBridge};
use crate::widgets::{installed_package_list, page_shell};

pub struct InstalledPage {
    pub root: GtkBox,
    list_host: GtkBox,
    bridge: UiBridge,
    request_id: std::rc::Rc<std::cell::Cell<u64>>,
}

impl InstalledPage {
    pub fn new(bridge: UiBridge) -> Self {
        let list_host = GtkBox::new(Orientation::Vertical, 0);
        list_host.set_hexpand(true);
        list_host.set_vexpand(true);
        let root = page_shell(&t("installed.title"), &list_host);
        let page = Self {
            root,
            list_host,
            bridge,
            request_id: Default::default(),
        };
        page.reload();
        page
    }

    pub fn reload(&self) {
        while let Some(child) = self.list_host.first_child() {
            self.list_host.remove(&child);
        }
        let spinner = Spinner::new();
        spinner.set_spinning(true);
        spinner.set_halign(gtk4::Align::Center);
        spinner.set_valign(gtk4::Align::Center);
        spinner.set_vexpand(true);
        self.list_host.append(&spinner);

        let list_host = self.list_host.clone();
        let bridge = self.bridge.clone();
        let request_id = self.request_id.clone();
        let id = request_id.get() + 1;
        request_id.set(id);
        self.bridge
            .store
            .fetch_async(ListKind::Installed, String::new(), move |listing| {
                if request_id.get() != id {
                    return;
                }
                while let Some(child) = list_host.first_child() {
                    list_host.remove(&child);
                }
                crate::widgets::append_listing_errors(&list_host, &listing.errors);
                let packages = listing.packages;
                if packages.is_empty() && !listing.errors.is_empty() {
                    return;
                }
                if packages.is_empty() {
                    let empty = libadwaita::StatusPage::builder()
                        .icon_name("view-grid-symbolic")
                        .title(t("installed.empty_title"))
                        .description(t("installed.empty_desc"))
                        .vexpand(true)
                        .build();
                    list_host.append(&empty);
                } else {
                    let label = gtk4::Label::builder()
                        .label(t_args(
                            "installed.count",
                            &[("n", &packages.len().to_string())],
                        ))
                        .halign(gtk4::Align::Start)
                        .css_classes(["dim-label"])
                        .build();
                    list_host.append(&label);
                    list_host.append(&installed_package_list(&packages, &bridge));
                }
            });
    }
}
