use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita::prelude::*;
use tcms_core::i18n::t;

use crate::icon_loader::IconLoader;
use crate::pages::{push_detail_page, ExplorePage, InstalledPage, SettingsPage, UpdatesPage};
use crate::store::{StoreService, UiBridge};

type ReloadHooks = Rc<RefCell<Vec<Box<dyn Fn()>>>>;
type OpenDetailFn = Rc<dyn Fn(tcms_core::Package)>;

pub struct StoreWindow {
    pub window: libadwaita::ApplicationWindow,
}

impl StoreWindow {
    pub fn new(app: &libadwaita::Application) -> Self {
        let store = StoreService::new();
        let icons = IconLoader::new(store.runtime());

        let window = libadwaita::ApplicationWindow::builder()
            .application(app)
            .title(t("app.name"))
            .default_width(1100)
            .default_height(720)
            .build();

        let toast_overlay = libadwaita::ToastOverlay::new();
        let navigation = libadwaita::NavigationView::new();
        let activity = libadwaita::Banner::builder()
            .title("")
            .revealed(false)
            .build();

        let reload_hooks: ReloadHooks = Rc::new(RefCell::new(Vec::new()));
        let window_gtk: gtk4::Window = window.clone().upcast();
        let busy_ops: Rc<RefCell<std::collections::HashSet<tcms_core::PackageId>>> =
            Rc::new(RefCell::new(std::collections::HashSet::new()));

        let make_bridge = {
            let store = store.clone();
            let toast = toast_overlay.clone();
            let reload_hooks = reload_hooks.clone();
            let window_gtk = window_gtk.clone();
            let icons = icons.clone();
            let busy_ops = busy_ops.clone();
            let activity = activity.clone();
            Rc::new(move |open_detail: OpenDetailFn| UiBridge {
                store: store.clone(),
                toast: toast.clone(),
                reload: {
                    let reload_hooks = reload_hooks.clone();
                    Rc::new(move || {
                        for hook in reload_hooks.borrow().iter() {
                            hook();
                        }
                    })
                },
                open_detail,
                window: window_gtk.clone(),
                icons: icons.clone(),
                busy: busy_ops.clone(),
                activity: activity.clone(),
            })
        };

        let open_detail: Rc<RefCell<Option<OpenDetailFn>>> = Rc::new(RefCell::new(None));

        let open_detail_fn = {
            let open_detail = open_detail.clone();
            let make_bridge = make_bridge.clone();
            let nav = navigation.clone();
            Rc::new(move |pkg: tcms_core::Package| {
                let open = open_detail
                    .borrow()
                    .clone()
                    .unwrap_or_else(|| Rc::new(|_| {}));
                let bridge = make_bridge(open);
                push_detail_page(&nav, &bridge, pkg);
            }) as OpenDetailFn
        };
        *open_detail.borrow_mut() = Some(open_detail_fn.clone());

        let bridge = make_bridge(open_detail_fn.clone());

        let view_stack = libadwaita::ViewStack::new();
        view_stack.set_vexpand(true);
        view_stack.set_hexpand(true);

        let explore = Rc::new(ExplorePage::new(bridge.clone()));
        let installed = Rc::new(InstalledPage::new(bridge.clone()));
        let updates = Rc::new(UpdatesPage::new(bridge.clone()));
        let settings = SettingsPage::new(store.clone());

        // Ctrl+F focuses Explore search.
        {
            let explore_focus = explore.clone();
            let view_stack_f = view_stack.clone();
            let controller = gtk4::EventControllerKey::new();
            controller.connect_key_pressed(move |_, key, _, modifier| {
                use gtk4::gdk::{Key, ModifierType};
                if modifier.contains(ModifierType::CONTROL_MASK)
                    && (key == Key::f || key == Key::F)
                {
                    view_stack_f.set_visible_child_name("explore");
                    explore_focus.focus_search();
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            });
            window.add_controller(controller);
        }

        {
            let mut hooks = reload_hooks.borrow_mut();
            let e = explore.clone();
            hooks.push(Box::new(move || e.reload()));
            let i = installed.clone();
            hooks.push(Box::new(move || i.reload()));
            let u = updates.clone();
            hooks.push(Box::new(move || u.reload()));
        }

        let explore_page = view_stack.add_titled(&explore.root, Some("explore"), &t("nav.explore"));
        explore_page.set_icon_name(Some("compass-symbolic"));
        let installed_page =
            view_stack.add_titled(&installed.root, Some("installed"), &t("nav.installed"));
        installed_page.set_icon_name(Some("view-grid-symbolic"));
        let updates_page = view_stack.add_titled(&updates.root, Some("updates"), &t("nav.updates"));
        updates_page.set_icon_name(Some("software-update-available-symbolic"));
        let settings_page =
            view_stack.add_titled(&settings.root, Some("settings"), &t("nav.settings"));
        settings_page.set_icon_name(Some("emblem-system-symbolic"));

        let switcher = libadwaita::ViewSwitcher::builder()
            .stack(&view_stack)
            .policy(libadwaita::ViewSwitcherPolicy::Wide)
            .build();

        let header = libadwaita::HeaderBar::builder()
            .title_widget(&switcher)
            .centering_policy(libadwaita::CenteringPolicy::Strict)
            .build();

        let menu_button = gtk4::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text(t("nav.menu"))
            .build();
        let menu = gio::Menu::new();
        menu.append(Some(&t("menu.about")), Some("app.about"));
        menu_button.set_menu_model(Some(&menu));
        header.pack_end(&menu_button);

        let refresh = gtk4::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text(t("menu.refresh"))
            .build();
        let toast_refresh = toast_overlay.clone();
        let store_refresh = store.clone();
        let reload_refresh = reload_hooks.clone();
        refresh.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            toast_refresh.add_toast(libadwaita::Toast::new(&t("toast.refreshing")));
            let btn = btn.clone();
            let toast_refresh = toast_refresh.clone();
            let reload_refresh = reload_refresh.clone();
            store_refresh.refresh_async(move |errors| {
                btn.set_sensitive(true);
                for hook in reload_refresh.borrow().iter() {
                    hook();
                }
                if errors.is_empty() {
                    toast_refresh.add_toast(libadwaita::Toast::new(&t("toast.refreshed")));
                } else {
                    toast_refresh.add_toast(libadwaita::Toast::new(&tcms_core::i18n::t_args(
                        "toast.refresh_failed",
                        &[("error", &errors.join("; "))],
                    )));
                }
            });
        });
        header.pack_start(&refresh);

        let toolbar = libadwaita::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.add_top_bar(&activity);
        toolbar.set_content(Some(&view_stack));

        let bottom_switcher = libadwaita::ViewSwitcherBar::builder()
            .stack(&view_stack)
            .reveal(false)
            .build();
        toolbar.add_bottom_bar(&bottom_switcher);

        let root_page = libadwaita::NavigationPage::builder()
            .title(t("app.name"))
            .child(&toolbar)
            .build();
        navigation.add(&root_page);

        toast_overlay.set_child(Some(&navigation));
        window.set_content(Some(&toast_overlay));

        let switcher_breakpoint = switcher.clone();
        let bottom_breakpoint = bottom_switcher.clone();
        let bp = libadwaita::Breakpoint::new(
            libadwaita::BreakpointCondition::parse("max-width: 700px").expect("breakpoint"),
        );
        bp.add_setter(
            &switcher_breakpoint,
            "policy",
            Some(&libadwaita::ViewSwitcherPolicy::Narrow.to_value()),
        );
        bp.connect_apply(move |_| {
            bottom_breakpoint.set_reveal(true);
        });
        let bottom_unapply = bottom_switcher.clone();
        bp.connect_unapply(move |_| {
            bottom_unapply.set_reveal(false);
        });
        window.add_breakpoint(bp);

        let about = gio::SimpleAction::new("about", None);
        let window_weak = window.downgrade();
        about.connect_activate(move |_, _| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let dialog = libadwaita::AboutDialog::builder()
                .application_name(t("app.name"))
                .application_icon("com.cursedmoon.Store")
                .developer_name(t("app.developer"))
                .version(env!("CARGO_PKG_VERSION"))
                .comments(t("about.comments"))
                .license_type(gtk4::License::Gpl30)
                .website("https://github.com/ayberkcn8181/the-cursed-moon-store")
                .build();
            dialog.present(Some(&window));
        });
        app.add_action(&about);

        let reload_lists = gio::SimpleAction::new("reload-lists", None);
        let reload_hooks_action = reload_hooks.clone();
        reload_lists.connect_activate(move |_, _| {
            for hook in reload_hooks_action.borrow().iter() {
                hook();
            }
        });
        app.add_action(&reload_lists);

        // Periodic update check while the store is open (General → automatic updates).
        {
            let store_check = store.clone();
            let toast_check = toast_overlay.clone();
            let updates_page = updates.clone();
            glib::timeout_add_local(std::time::Duration::from_secs(30 * 60), move || {
                if !store_check.config().automatic_updates_check {
                    return glib::ControlFlow::Continue;
                }
                let store_check = store_check.clone();
                let toast_check = toast_check.clone();
                let updates_page = updates_page.clone();
                store_check.fetch_async(
                    crate::store::ListKind::Updates,
                    String::new(),
                    move |pkgs| {
                        if pkgs.is_empty() {
                            return;
                        }
                        toast_check.add_toast(libadwaita::Toast::new(&tcms_core::i18n::t_args(
                            "toast.updates_available",
                            &[("n", &pkgs.len().to_string())],
                        )));
                        updates_page.reload();
                    },
                );
                glib::ControlFlow::Continue
            });
        }

        window.connect_destroy(move |_| {
            let _keep = (&explore, &installed, &updates);
        });

        Self { window }
    }

    pub fn present(&self) {
        self.window.present();
    }
}
