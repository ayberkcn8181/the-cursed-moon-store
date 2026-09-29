use gtk4::prelude::*;

pub fn context(display: Option<&gtk4::gdk::Display>) -> gio::AppLaunchContext {
    let context: gio::AppLaunchContext = display
        .map(|display| display.app_launch_context().upcast())
        .unwrap_or_default();
    for (key, value) in tcms_core::host_environment::overrides() {
        match value {
            Some(value) => context.setenv(key, value),
            None => context.unsetenv(key),
        }
    }
    context
}

pub fn subprocess_launcher() -> gio::SubprocessLauncher {
    let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::NONE);
    for (key, value) in tcms_core::host_environment::overrides() {
        match value {
            Some(value) => launcher.setenv(key, value, true),
            None => launcher.unsetenv(key),
        }
    }
    launcher
}
