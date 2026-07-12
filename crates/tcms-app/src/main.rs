mod icon_loader;
mod pages;
mod store;
mod widgets;
mod window;

use anyhow::Result;
use gtk4::prelude::*;
use tcms_core::i18n;
use tracing_subscriber::EnvFilter;

use crate::window::StoreWindow;

const APP_ID: &str = "com.cursedmoon.Store";

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // Resolve language before building any UI.
    let config = tcms_core::AppConfig::load().unwrap_or_default();
    let lang = i18n::resolve(&config.language);
    i18n::set_current(lang);

    let app = libadwaita::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::FLAGS_NONE)
        .build();

    app.connect_activate(|app| {
        // Replace existing windows when reloading UI (e.g. language change).
        for win in app.windows() {
            win.close();
        }
        let window = StoreWindow::new(app);
        window.present();
    });

    let reload_ui = gio::SimpleAction::new("reload-ui", None);
    let app_weak = app.downgrade();
    reload_ui.connect_activate(move |_, _| {
        if let Some(app) = app_weak.upgrade() {
            app.activate();
        }
    });
    app.add_action(&reload_ui);

    app.run();
    Ok(())
}
