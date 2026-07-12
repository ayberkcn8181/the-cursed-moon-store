use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Orientation, SearchEntry, Spinner};

use tcms_core::i18n::t;
use tcms_core::Package;

use crate::store::{ListKind, UiBridge};
use crate::widgets::{featured_view, package_list, page_shell};

const CATEGORIES: &[(&str, &str)] = &[
    ("", "category.all"),
    ("Game", "category.games"),
    ("AudioVideo", "category.multimedia"),
    ("Graphics", "category.graphics"),
    ("Office", "category.office"),
    ("Network", "category.internet"),
    ("Development", "category.development"),
    ("Utility", "category.utilities"),
    ("System", "category.system"),
    ("Education", "category.education"),
];

pub struct ExplorePage {
    pub root: GtkBox,
    list_host: GtkBox,
    search: SearchEntry,
    bridge: UiBridge,
    request_id: Rc<Cell<u64>>,
    category: Rc<RefCell<String>>,
}

impl ExplorePage {
    pub fn new(bridge: UiBridge) -> Self {
        let container = GtkBox::new(Orientation::Vertical, 12);
        container.set_hexpand(true);
        container.set_vexpand(true);

        let search = SearchEntry::builder()
            .placeholder_text(t("explore.search_placeholder"))
            .hexpand(true)
            .build();

        let list_host = GtkBox::new(Orientation::Vertical, 0);
        list_host.set_hexpand(true);
        list_host.set_vexpand(true);

        let category = Rc::new(RefCell::new(String::new()));
        let request_id = Rc::new(Cell::new(0));

        let chips = GtkBox::new(Orientation::Horizontal, 6);
        chips.set_halign(gtk4::Align::Start);
        let mut first_btn: Option<gtk4::ToggleButton> = None;
        for (id, key) in CATEGORIES {
            let btn = gtk4::ToggleButton::builder()
                .label(t(key))
                .css_classes(["pill"])
                .build();
            if let Some(ref group) = first_btn {
                btn.set_group(Some(group));
            } else {
                btn.set_active(true);
                first_btn = Some(btn.clone());
            }
            let cat = category.clone();
            let cat_id = (*id).to_string();
            let list_host2 = list_host.clone();
            let bridge2 = bridge.clone();
            let search2 = search.clone();
            let request_id2 = request_id.clone();
            btn.connect_toggled(move |btn| {
                if !btn.is_active() {
                    return;
                }
                *cat.borrow_mut() = cat_id.clone();
                refresh_explore(
                    &list_host2,
                    &bridge2,
                    &request_id2,
                    &search2.text(),
                    &cat_id,
                );
            });
            chips.append(&btn);
        }

        let chips_scroll = gtk4::ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Automatic)
            .vscrollbar_policy(gtk4::PolicyType::Never)
            .hexpand(true)
            .child(&chips)
            .build();
        chips_scroll.set_height_request(40);

        container.append(&search);
        container.append(&chips_scroll);
        container.append(&list_host);

        let root = page_shell(&t("explore.title"), &container);
        let debounce = Rc::new(Cell::new(0));
        let page = Self {
            root,
            list_host: list_host.clone(),
            search: search.clone(),
            bridge: bridge.clone(),
            request_id: request_id.clone(),
            category: category.clone(),
        };
        page.show_featured();

        let category_s = category.clone();
        search.connect_search_changed(move |entry| {
            let text = entry.text().to_string();
            let tick = debounce.get() + 1;
            debounce.set(tick);
            let cat = category_s.borrow().clone();
            let list_host2 = list_host.clone();
            let bridge2 = bridge.clone();
            let request_id2 = request_id.clone();
            let debounce2 = debounce.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(320), move || {
                if debounce2.get() != tick {
                    return;
                }
                refresh_explore(&list_host2, &bridge2, &request_id2, &text, &cat);
            });
        });

        page
    }

    fn show_featured(&self) {
        show_featured_into(&self.list_host, &self.bridge, &self.request_id);
    }

    pub fn reload(&self) {
        refresh_explore(
            &self.list_host,
            &self.bridge,
            &self.request_id,
            &self.search.text(),
            &self.category.borrow(),
        );
    }

    pub fn focus_search(&self) {
        self.search.grab_focus();
    }
}

fn refresh_explore(
    list_host: &GtkBox,
    bridge: &UiBridge,
    request_id: &Rc<Cell<u64>>,
    text: &str,
    category: &str,
) {
    if text.trim().is_empty() && category.is_empty() {
        show_featured_into(list_host, bridge, request_id);
        return;
    }
    let query = if text.trim().is_empty() {
        category.to_string()
    } else {
        text.to_string()
    };
    run_search(list_host, bridge, request_id, query, category.to_string());
}

fn clear_host(list_host: &GtkBox) {
    while let Some(child) = list_host.first_child() {
        list_host.remove(&child);
    }
}

fn show_spinner(list_host: &GtkBox) {
    clear_host(list_host);
    let spinner = Spinner::new();
    spinner.set_spinning(true);
    spinner.set_halign(gtk4::Align::Center);
    spinner.set_valign(gtk4::Align::Center);
    spinner.set_vexpand(true);
    list_host.append(&spinner);
}

fn show_featured_into(list_host: &GtkBox, bridge: &UiBridge, request_id: &Rc<Cell<u64>>) {
    clear_host(list_host);
    let loading = libadwaita::StatusPage::builder()
        .icon_name("emblem-favorite-symbolic")
        .title(t("featured.loading"))
        .description(t("featured.empty_desc"))
        .vexpand(true)
        .build();
    list_host.append(&loading);

    let id = request_id.get() + 1;
    request_id.set(id);
    let list_host2 = list_host.clone();
    let bridge2 = bridge.clone();
    let request_id2 = request_id.clone();
    bridge.store.fetch_featured_async(move |sections| {
        if request_id2.get() != id {
            return;
        }
        clear_host(&list_host2);
        list_host2.append(&featured_view(&sections, &bridge2));
    });
}

fn filter_by_category(packages: Vec<Package>, category: &str) -> Vec<Package> {
    if category.is_empty() {
        return packages;
    }
    let cat = category.to_ascii_lowercase();
    packages
        .into_iter()
        .filter(|p| {
            p.categories
                .iter()
                .any(|c| c.to_ascii_lowercase().contains(&cat))
                || p.id.id.to_ascii_lowercase().contains(&cat)
                || p.summary.to_ascii_lowercase().contains(&cat)
                || p.name.to_ascii_lowercase().contains(&cat)
        })
        .collect()
}

fn run_search(
    list_host: &GtkBox,
    bridge: &UiBridge,
    request_id: &Rc<Cell<u64>>,
    text: String,
    category: String,
) {
    show_spinner(list_host);
    let id = request_id.get() + 1;
    request_id.set(id);
    let list_host2 = list_host.clone();
    let bridge2 = bridge.clone();
    let request_id2 = request_id.clone();
    bridge
        .store
        .fetch_async(ListKind::Explore, text, move |packages| {
            if request_id2.get() != id {
                return;
            }
            let packages = filter_by_category(packages, &category);
            if packages.len() >= 40 {
                bridge2.toast_msg(&t("explore.truncated"));
            }
            clear_host(&list_host2);
            list_host2.append(&package_list(&packages, &bridge2));
        });
}
