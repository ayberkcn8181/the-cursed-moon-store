use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Entry, Label, Orientation, PolicyType, ScrolledWindow, Switch, TextView,
};
use libadwaita::prelude::*;
use tcms_core::i18n::{self, t, Language};
use tcms_core::AppConfig;

use crate::store::StoreService;
use crate::widgets::page_shell;

pub struct SettingsPage {
    pub root: GtkBox,
}

impl SettingsPage {
    pub fn new(store: StoreService) -> Self {
        let config = store.config();

        let view_stack = libadwaita::ViewStack::new();
        view_stack.add_titled(
            &general_page(&store, &config),
            Some("general"),
            &t("settings.general"),
        );
        view_stack.add_titled(
            &sources_page(&store, &config),
            Some("sources"),
            &t("settings.sources"),
        );
        view_stack.add_titled(
            &advanced_page(&store, &config),
            Some("advanced"),
            &t("settings.advanced"),
        );

        let switcher = libadwaita::ViewSwitcher::builder()
            .stack(&view_stack)
            .policy(libadwaita::ViewSwitcherPolicy::Wide)
            .halign(Align::Center)
            .build();

        let content = GtkBox::new(Orientation::Vertical, 18);
        content.set_hexpand(true);
        content.set_vexpand(true);
        content.append(&switcher);
        content.append(&view_stack);

        let root = page_shell(&t("settings.title"), &content);
        Self { root }
    }
}

fn general_page(store: &StoreService, config: &AppConfig) -> ScrolledWindow {
    let list = GtkBox::new(Orientation::Vertical, 12);

    // Language — next to general preferences.
    let lang_group = libadwaita::PreferencesGroup::builder()
        .title(t("settings.language"))
        .description(t("settings.language_desc"))
        .build();

    let lang_row = libadwaita::ComboRow::builder()
        .title(t("settings.language"))
        .build();
    let mut labels: Vec<String> = vec![t("settings.language_system")];
    labels.extend(Language::ALL.iter().map(|l| l.native_name().to_string()));
    let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let model = gtk4::StringList::new(&label_refs);
    lang_row.set_model(Some(&model));

    let selected = if config.language.trim().eq_ignore_ascii_case("system")
        || config.language.trim().is_empty()
    {
        0
    } else {
        Language::ALL
            .iter()
            .position(|l| l.code() == i18n::resolve(&config.language).code())
            .map(|i| i + 1)
            .unwrap_or(0)
    } as u32;
    lang_row.set_selected(selected);

    let store_lang = store.clone();
    lang_row.connect_selected_notify(move |row| {
        let idx = row.selected() as usize;
        let (code, lang) = if idx == 0 {
            ("system".to_string(), Language::from_system())
        } else {
            let Some(lang) = Language::ALL.get(idx - 1).copied() else {
                return;
            };
            (lang.code().to_string(), lang)
        };
        if lang == i18n::current() && store_lang.config().language == code {
            return;
        }
        i18n::set_current(lang);
        let mut cfg = store_lang.config();
        cfg.language = code;
        let _ = store_lang.save_config(cfg);
        if let Some(app) = gio::Application::default() {
            app.activate_action("reload-ui", None);
        }
    });
    lang_group.add(&lang_row);
    list.append(&lang_group);

    let visibility = libadwaita::PreferencesGroup::builder()
        .title(t("settings.visibility_group"))
        .description(t("settings.visibility_group_desc"))
        .build();
    let codecs = switch_row(
        &t("settings.show_codecs"),
        &t("settings.show_codecs_desc"),
        config.show_codecs,
    );
    let drivers = switch_row(
        &t("settings.show_drivers"),
        &t("settings.show_drivers_desc"),
        config.show_drivers,
    );
    let system = switch_row(
        &t("settings.show_system"),
        &t("settings.show_system_desc"),
        config.show_system_packages,
    );
    visibility.add(&codecs.0);
    visibility.add(&drivers.0);
    visibility.add(&system.0);

    let store_codecs = store.clone();
    codecs.1.connect_active_notify(move |sw| {
        let mut cfg = store_codecs.config();
        cfg.show_codecs = sw.is_active();
        let _ = store_codecs.save_config(cfg);
        if let Some(app) = gio::Application::default() {
            app.activate_action("reload-lists", None);
        }
    });
    let store_drivers = store.clone();
    drivers.1.connect_active_notify(move |sw| {
        let mut cfg = store_drivers.config();
        cfg.show_drivers = sw.is_active();
        let _ = store_drivers.save_config(cfg);
        if let Some(app) = gio::Application::default() {
            app.activate_action("reload-lists", None);
        }
    });
    let store_system = store.clone();
    system.1.connect_active_notify(move |sw| {
        let mut cfg = store_system.config();
        cfg.show_system_packages = sw.is_active();
        let _ = store_system.save_config(cfg);
        if let Some(app) = gio::Application::default() {
            app.activate_action("reload-lists", None);
        }
    });
    list.append(&visibility);

    let group = libadwaita::PreferencesGroup::builder()
        .title(t("settings.updates_group"))
        .description(t("settings.updates_group_desc"))
        .build();

    let auto = switch_row(
        &t("settings.auto_updates"),
        &t("settings.auto_updates_desc"),
        config.automatic_updates_check,
    );
    let bg = switch_row(
        &t("settings.bg_download"),
        &t("settings.bg_download_desc"),
        config.download_updates_in_background,
    );
    // Background download is not implemented yet — keep the preference for future use.
    bg.1.set_sensitive(false);
    group.add(&auto.0);
    group.add(&bg.0);

    let store_auto = store.clone();
    auto.1.connect_active_notify(move |sw| {
        let mut cfg = store_auto.config();
        cfg.automatic_updates_check = sw.is_active();
        let _ = store_auto.save_config(cfg);
    });
    let store_bg = store.clone();
    bg.1.connect_active_notify(move |sw| {
        let mut cfg = store_bg.config();
        cfg.download_updates_in_background = sw.is_active();
        let _ = store_bg.save_config(cfg);
    });

    list.append(&group);
    wrap_scroll(&list)
}

fn sources_page(store: &StoreService, config: &AppConfig) -> ScrolledWindow {
    let list = GtkBox::new(Orientation::Vertical, 12);

    let group = libadwaita::PreferencesGroup::builder()
        .title(t("settings.sources_group"))
        .description(t("settings.sources_group_desc"))
        .build();

    let pacman = switch_row(
        &t("settings.source_pacman"),
        &t("settings.source_pacman_desc"),
        config.enable_pacman,
    );
    let flatpak = switch_row(
        &t("settings.source_flatpak"),
        &t("settings.source_flatpak_desc"),
        config.enable_flatpak,
    );
    let aur = switch_row(
        &t("settings.source_aur"),
        &t("settings.source_aur_desc"),
        config.enable_aur,
    );
    group.add(&pacman.0);
    group.add(&flatpak.0);
    group.add(&aur.0);

    bind_source_switch(store, &pacman.1, |cfg, v| cfg.enable_pacman = v);
    bind_source_switch(store, &flatpak.1, |cfg, v| cfg.enable_flatpak = v);
    bind_source_switch(store, &aur.1, |cfg, v| cfg.enable_aur = v);

    list.append(&group);
    wrap_scroll(&list)
}

fn advanced_page(store: &StoreService, config: &AppConfig) -> ScrolledWindow {
    let list = GtkBox::new(Orientation::Vertical, 18);

    let warning = libadwaita::Banner::builder()
        .title(t("advanced.warning"))
        .revealed(true)
        .build();
    list.append(&warning);

    let install_group = libadwaita::PreferencesGroup::builder()
        .title(t("advanced.install_group"))
        .description(t("advanced.install_group_desc"))
        .build();

    let priority_row = libadwaita::ComboRow::builder()
        .title(t("advanced.install_priority"))
        .subtitle(t("advanced.install_priority_desc"))
        .build();
    let priority_options: Vec<(String, String)> = vec![
        ("pacman,flatpak,aur".into(), t("advanced.priority_pfa")),
        ("pacman,aur,flatpak".into(), t("advanced.priority_paf")),
        ("flatpak,pacman,aur".into(), t("advanced.priority_fpa")),
        ("flatpak,aur,pacman".into(), t("advanced.priority_fap")),
        ("aur,pacman,flatpak".into(), t("advanced.priority_apf")),
        ("aur,flatpak,pacman".into(), t("advanced.priority_afp")),
    ];
    let label_refs: Vec<&str> = priority_options.iter().map(|(_, l)| l.as_str()).collect();
    let model = gtk4::StringList::new(&label_refs);
    priority_row.set_model(Some(&model));
    let current_key = config.advanced.install_source_priority.join(",");
    let selected = priority_options
        .iter()
        .position(|(k, _)| *k == current_key)
        .unwrap_or(0) as u32;
    priority_row.set_selected(selected);

    let ask_repo = switch_row(
        &t("advanced.ask_repo"),
        &t("advanced.ask_repo_desc"),
        config.advanced.ask_repo_on_install,
    );
    install_group.add(&priority_row);
    install_group.add(&ask_repo.0);
    list.append(&install_group);

    // Persist install prefs immediately (no need to hit Save).
    {
        let store_p = store.clone();
        let keys: Vec<String> = priority_options.iter().map(|(k, _)| k.clone()).collect();
        priority_row.connect_selected_notify(move |row| {
            let idx = row.selected() as usize;
            let Some(key) = keys.get(idx) else {
                return;
            };
            let mut cfg = store_p.config();
            let next: Vec<String> = key.split(',').map(|s| s.to_string()).collect();
            if cfg.advanced.install_source_priority == next {
                return;
            }
            cfg.advanced.install_source_priority = next;
            let _ = store_p.save_config(cfg);
        });
    }
    {
        let store_a = store.clone();
        ask_repo.1.connect_active_notify(move |sw| {
            let mut cfg = store_a.config();
            if cfg.advanced.ask_repo_on_install == sw.is_active() {
                return;
            }
            cfg.advanced.ask_repo_on_install = sw.is_active();
            let _ = store_a.save_config(cfg);
        });
    }

    let pacman = libadwaita::PreferencesGroup::builder()
        .title(t("advanced.pacman"))
        .description(t("advanced.pacman_desc"))
        .build();
    let pacman_conf = entry_row(
        &t("advanced.pacman_conf"),
        &t("advanced.pacman_conf_desc"),
        &config.advanced.pacman_conf,
    );
    let pacman_args = entry_row(
        &t("advanced.pacman_args"),
        &t("advanced.pacman_args_desc"),
        &config.advanced.pacman_extra_args,
    );
    pacman.add(&pacman_conf.0);
    pacman.add(&pacman_args.0);
    list.append(&pacman);

    let flatpak = libadwaita::PreferencesGroup::builder()
        .title(t("advanced.flatpak"))
        .description(t("advanced.flatpak_desc"))
        .build();
    let flatpak_inst = entry_row(
        &t("advanced.flatpak_install"),
        &t("advanced.flatpak_install_desc"),
        &config.advanced.flatpak_installation,
    );
    flatpak.add(&flatpak_inst.0);

    let remotes_label = Label::builder()
        .label(t("advanced.flatpak_remotes"))
        .halign(Align::Start)
        .css_classes(["heading"])
        .margin_top(6)
        .build();
    let remotes = TextView::builder()
        .monospace(true)
        .wrap_mode(gtk4::WrapMode::WordChar)
        .build();
    remotes.buffer().set_text(&config.advanced.flatpak_remotes);
    let remotes_frame = gtk4::Frame::builder().child(&remotes).build();
    remotes_frame.set_height_request(100);
    flatpak.add(&remotes_label);
    flatpak.add(&remotes_frame);
    list.append(&flatpak);

    let aur = libadwaita::PreferencesGroup::builder()
        .title(t("advanced.aur"))
        .description(t("advanced.aur_desc"))
        .build();
    let aur_rpc = entry_row(
        &t("advanced.aur_rpc"),
        &t("advanced.aur_rpc_desc"),
        &config.advanced.aur_rpc_url,
    );
    let aur_helper = entry_row(
        &t("advanced.aur_helper"),
        &t("advanced.aur_helper_desc"),
        &config.advanced.aur_helper,
    );
    let aur_args = entry_row(
        &t("advanced.aur_args"),
        &t("advanced.aur_args_desc"),
        &config.advanced.aur_extra_args,
    );
    aur.add(&aur_rpc.0);
    aur.add(&aur_helper.0);
    aur.add(&aur_args.0);
    list.append(&aur);

    let raw_group = libadwaita::PreferencesGroup::builder()
        .title(t("advanced.raw_group"))
        .description(t("advanced.raw_group_desc"))
        .build();
    let allow_raw = switch_row(
        &t("advanced.allow_raw"),
        &t("advanced.allow_raw_desc"),
        config.advanced.allow_raw_config_edit,
    );
    raw_group.add(&allow_raw.0);

    let overlay_label = Label::builder()
        .label(t("advanced.raw_overlay"))
        .halign(Align::Start)
        .css_classes(["heading"])
        .margin_top(6)
        .build();
    let overlay_hint = Label::builder()
        .label(t("advanced.raw_overlay_hint"))
        .halign(Align::Start)
        .wrap(true)
        .css_classes(["dim-label", "caption"])
        .build();
    let overlay = TextView::builder()
        .monospace(true)
        .wrap_mode(gtk4::WrapMode::WordChar)
        .accepts_tab(true)
        .build();
    overlay.buffer().set_text(&config.advanced.raw_overlay);
    let overlay_frame = gtk4::Frame::builder().child(&overlay).build();
    overlay_frame.set_height_request(120);
    raw_group.add(&overlay_label);
    raw_group.add(&overlay_hint);
    raw_group.add(&overlay_frame);

    let full_label = Label::builder()
        .label(t("advanced.full_config"))
        .halign(Align::Start)
        .css_classes(["heading"])
        .margin_top(12)
        .build();
    let full_editor = TextView::builder()
        .monospace(true)
        .wrap_mode(gtk4::WrapMode::WordChar)
        .accepts_tab(true)
        .editable(config.advanced.allow_raw_config_edit)
        .build();
    if let Ok(text) = toml::to_string_pretty(config) {
        full_editor.buffer().set_text(&text);
    }
    let full_frame = gtk4::Frame::builder().child(&full_editor).build();
    full_frame.set_height_request(220);
    raw_group.add(&full_label);
    raw_group.add(&full_frame);
    list.append(&raw_group);

    let actions = GtkBox::new(Orientation::Horizontal, 12);
    actions.set_halign(Align::End);
    let reset = gtk4::Button::builder()
        .label(t("advanced.reset"))
        .css_classes(["destructive-action"])
        .build();
    let save = gtk4::Button::builder()
        .label(t("advanced.save"))
        .css_classes(["suggested-action", "pill"])
        .build();
    actions.append(&reset);
    actions.append(&save);
    list.append(&actions);

    let store_save = store.clone();
    let pacman_conf_e = pacman_conf.1.clone();
    let pacman_args_e = pacman_args.1.clone();
    let flatpak_inst_e = flatpak_inst.1.clone();
    let remotes_tv = remotes.clone();
    let aur_rpc_e = aur_rpc.1.clone();
    let aur_helper_e = aur_helper.1.clone();
    let aur_args_e = aur_args.1.clone();
    let overlay_tv = overlay.clone();
    let full_tv = full_editor.clone();
    let allow_sw = allow_raw.1.clone();
    let ask_sw = ask_repo.1.clone();
    let priority_combo = priority_row.clone();
    let priority_keys: Vec<String> = priority_options.iter().map(|(k, _)| k.clone()).collect();

    save.connect_clicked(move |_| {
        let mut cfg = store_save.config();

        if allow_sw.is_active() {
            let buffer = full_tv.buffer();
            let start = buffer.start_iter();
            let end = buffer.end_iter();
            let text = buffer.text(&start, &end, false);
            if let Ok(parsed) = toml::from_str::<AppConfig>(&text) {
                match store_save.save_config(parsed) {
                    Ok(()) => {
                        notify_settings_saved(true, None);
                        if let Some(app) = gio::Application::default() {
                            app.activate_action("reload-lists", None);
                        }
                        return;
                    }
                    Err(err) => {
                        notify_settings_saved(false, Some(err.to_string()));
                        return;
                    }
                }
            }
        }

        cfg.advanced.pacman_conf = pacman_conf_e.text().to_string();
        cfg.advanced.pacman_extra_args = pacman_args_e.text().to_string();
        cfg.advanced.flatpak_installation = flatpak_inst_e.text().to_string();
        {
            let buffer = remotes_tv.buffer();
            let start = buffer.start_iter();
            let end = buffer.end_iter();
            cfg.advanced.flatpak_remotes = buffer.text(&start, &end, false).to_string();
        }
        cfg.advanced.aur_rpc_url = aur_rpc_e.text().to_string();
        cfg.advanced.aur_helper = aur_helper_e.text().to_string();
        cfg.advanced.aur_extra_args = aur_args_e.text().to_string();
        cfg.advanced.allow_raw_config_edit = allow_sw.is_active();
        cfg.advanced.ask_repo_on_install = ask_sw.is_active();
        let idx = priority_combo.selected() as usize;
        if let Some(key) = priority_keys.get(idx) {
            cfg.advanced.install_source_priority = key.split(',').map(|s| s.to_string()).collect();
        }
        {
            let buffer = overlay_tv.buffer();
            let start = buffer.start_iter();
            let end = buffer.end_iter();
            cfg.advanced.raw_overlay = buffer.text(&start, &end, false).to_string();
        }

        match store_save.save_config(cfg) {
            Ok(()) => {
                notify_settings_saved(true, None);
                if let Some(app) = gio::Application::default() {
                    app.activate_action("reload-lists", None);
                }
            }
            Err(err) => notify_settings_saved(false, Some(err.to_string())),
        }
    });

    let store_reset = store.clone();
    let pacman_conf_r = pacman_conf.1.clone();
    let pacman_args_r = pacman_args.1.clone();
    let flatpak_inst_r = flatpak_inst.1.clone();
    let remotes_r = remotes.clone();
    let aur_rpc_r = aur_rpc.1.clone();
    let aur_helper_r = aur_helper.1.clone();
    let aur_args_r = aur_args.1.clone();
    let overlay_r = overlay.clone();
    let full_r = full_editor.clone();
    let allow_r = allow_raw.1.clone();
    let ask_r = ask_repo.1.clone();
    let priority_r = priority_row.clone();

    reset.connect_clicked(move |_| {
        let mut cfg = store_reset.config();
        cfg.advanced = tcms_core::AdvancedConfig::default();
        let _ = store_reset.save_config(cfg.clone());
        pacman_conf_r.set_text(&cfg.advanced.pacman_conf);
        pacman_args_r.set_text(&cfg.advanced.pacman_extra_args);
        flatpak_inst_r.set_text(&cfg.advanced.flatpak_installation);
        remotes_r.buffer().set_text(&cfg.advanced.flatpak_remotes);
        aur_rpc_r.set_text(&cfg.advanced.aur_rpc_url);
        aur_helper_r.set_text(&cfg.advanced.aur_helper);
        aur_args_r.set_text(&cfg.advanced.aur_extra_args);
        allow_r.set_active(cfg.advanced.allow_raw_config_edit);
        ask_r.set_active(cfg.advanced.ask_repo_on_install);
        priority_r.set_selected(0);
        overlay_r.buffer().set_text(&cfg.advanced.raw_overlay);
        if let Ok(text) = toml::to_string_pretty(&cfg) {
            full_r.buffer().set_text(&text);
        }
    });

    let full_edit = full_editor.clone();
    allow_raw.1.connect_active_notify(move |sw| {
        full_edit.set_editable(sw.is_active());
    });

    wrap_scroll(&list)
}

fn switch_row(title: &str, subtitle: &str, active: bool) -> (libadwaita::ActionRow, Switch) {
    let row = libadwaita::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .build();
    let sw = Switch::builder()
        .active(active)
        .valign(Align::Center)
        .build();
    row.add_suffix(&sw);
    row.set_activatable_widget(Some(&sw));
    (row, sw)
}

fn entry_row(title: &str, subtitle: &str, text: &str) -> (libadwaita::ActionRow, Entry) {
    let row = libadwaita::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .build();
    let entry = Entry::builder()
        .text(text)
        .hexpand(true)
        .valign(Align::Center)
        .width_chars(28)
        .build();
    row.add_suffix(&entry);
    (row, entry)
}

fn bind_source_switch(
    store: &StoreService,
    sw: &Switch,
    setter: impl Fn(&mut AppConfig, bool) + 'static,
) {
    let store = store.clone();
    sw.connect_active_notify(move |sw| {
        let mut cfg = store.config();
        setter(&mut cfg, sw.is_active());
        let _ = store.save_config(cfg);
        if let Some(app) = gio::Application::default() {
            app.activate_action("reload-lists", None);
        }
    });
}

fn wrap_scroll(child: &impl IsA<gtk4::Widget>) -> ScrolledWindow {
    ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .child(child)
        .build()
}

fn notify_settings_saved(ok: bool, error: Option<String>) {
    let dialog = if ok {
        libadwaita::AlertDialog::builder()
            .heading(t("advanced.saved"))
            .body(t("advanced.saved_body"))
            .build()
    } else {
        let err = error.unwrap_or_default();
        libadwaita::AlertDialog::builder()
            .heading(t("advanced.save_failed_title"))
            .body(tcms_core::i18n::t_args(
                "advanced.save_failed",
                &[("error", &err)],
            ))
            .build()
    };
    dialog.add_response("ok", &t("action.ok"));
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("ok");
    if let Some(app) = gio::Application::default() {
        if let Ok(app) = app.downcast::<gtk4::Application>() {
            if let Some(win) = app.active_window() {
                dialog.present(Some(&win));
                return;
            }
        }
    }
    dialog.present(None::<&gtk4::Window>);
}
