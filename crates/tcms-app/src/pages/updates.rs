use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Orientation, Spinner};
use std::cell::Cell;
use std::rc::Rc;

use tcms_core::i18n::{t, t_args};

use crate::store::UiBridge;
use crate::widgets::{package_list, page_shell};

pub struct UpdatesPage {
    pub root: GtkBox,
    list_host: GtkBox,
    bridge: UiBridge,
    request_id: Rc<Cell<u64>>,
}

impl UpdatesPage {
    pub fn new(bridge: UiBridge) -> Self {
        let list_host = GtkBox::new(Orientation::Vertical, 12);
        list_host.set_hexpand(true);
        list_host.set_vexpand(true);
        let root = page_shell(&t("updates.title"), &list_host);
        let page = Self {
            root,
            list_host,
            bridge,
            request_id: Rc::new(Cell::new(0)),
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
        self.bridge.store.fetch_updates_async(move |listing| {
            if request_id.get() != id {
                return;
            }
            while let Some(child) = list_host.first_child() {
                list_host.remove(&child);
            }

            if !listing.errors.is_empty() {
                let errors = gtk4::Label::builder()
                    .label(format!(
                        "{}\n{}",
                        t("updates.check_failed"),
                        listing.errors.join("\n")
                    ))
                    .wrap(true)
                    .xalign(0.0)
                    .selectable(true)
                    .css_classes(["error"])
                    .build();
                list_host.append(&errors);
            }
            let packages = listing.packages;
            if packages.is_empty() && !listing.errors.is_empty() {
                return;
            }
            if packages.is_empty() {
                let empty = libadwaita::StatusPage::builder()
                    .icon_name("emblem-ok-symbolic")
                    .title(t("updates.up_to_date_title"))
                    .description(t("updates.up_to_date_desc"))
                    .vexpand(true)
                    .build();
                list_host.append(&empty);
                return;
            }

            let header = GtkBox::new(Orientation::Horizontal, 12);
            let summary = gtk4::Label::builder()
                .label(t_args(
                    "updates.count",
                    &[("n", &packages.len().to_string())],
                ))
                .halign(gtk4::Align::Start)
                .hexpand(true)
                .css_classes(["heading"])
                .build();
            let update_all = gtk4::Button::builder()
                .label(t("updates.update_all"))
                .halign(gtk4::Align::End)
                .css_classes(["suggested-action", "pill"])
                .build();

            let bridge_ua = bridge.clone();
            update_all.connect_clicked(move |btn| {
                use libadwaita::prelude::*;
                let dialog = libadwaita::AlertDialog::builder()
                    .heading(t("confirm.update_all_title"))
                    .body(t("confirm.update_all_body"))
                    .build();
                dialog.add_response("cancel", &t("action.cancel"));
                dialog.add_response("update", &t("updates.update_all"));
                dialog.set_response_appearance("update", libadwaita::ResponseAppearance::Suggested);
                dialog.set_default_response(Some("update"));
                dialog.set_close_response("cancel");
                let bridge_ua = bridge_ua.clone();
                let btn = btn.clone();
                let window = bridge_ua.window.clone();
                dialog.connect_response(None, move |_, response| {
                    if response != "update" {
                        return;
                    }
                    btn.set_sensitive(false);
                    bridge_ua.toast_msg(&t("updates.updating_all"));
                    bridge_ua.package_started(&t("updates.updating_all"));
                    let bridge2 = bridge_ua.clone();
                    let btn = btn.clone();
                    bridge_ua.store.update_all_async(
                        {
                            let bridge = bridge_ua.clone();
                            move |text| bridge.append_progress(text)
                        },
                        move |report| {
                            btn.set_sensitive(true);
                            bridge2.package_finished();
                            let completed = report.completed.join(", ");
                            let mut body =
                                t_args("updates.completed_sources", &[("sources", &completed)]);
                            if !report.errors.is_empty() {
                                body.push_str("\n\n");
                                body.push_str(&report.errors.join("\n"));
                            }
                            let result_dialog = libadwaita::AlertDialog::builder()
                                .heading(t(if report.errors.is_empty() {
                                    "updates.finished"
                                } else {
                                    "updates.partial_failure"
                                }))
                                .body(body)
                                .build();
                            result_dialog.add_response("close", &t("action.close"));
                            result_dialog.present(Some(&bridge2.window));
                            (bridge2.reload)();
                        },
                    );
                });
                dialog.present(Some(&window));
            });

            header.append(&summary);
            header.append(&update_all);
            list_host.append(&header);
            list_host.append(&package_list(&packages, &bridge));
        });
    }
}
