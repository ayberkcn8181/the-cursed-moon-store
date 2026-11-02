//! Read-only previews and explicitly confirmed management operations.
use crate::store::UiBridge;
use gtk4::prelude::*;
use libadwaita::prelude::*;
use tcms_core::{t, FlatpakInstallation, PackageId};
#[derive(Clone)]
pub enum CleanupKind {
    Orphans,
    Cache,
    Flatpak(FlatpakInstallation),
}
#[derive(Clone)]
pub enum Operation {
    Permissions(tcms_flatpak::permissions::Change),
    Orphans(Vec<tcms_pacman::maintenance::Orphan>),
    Cache(tcms_pacman::maintenance::CachePlan),
    Flatpak(
        FlatpakInstallation,
        Vec<tcms_flatpak::maintenance::UnusedRef>,
    ),
}
impl CleanupKind {
    pub fn title(&self) -> String {
        t(match self {
            Self::Orphans => "maintenance.orphans",
            Self::Cache => "maintenance.cache",
            Self::Flatpak(FlatpakInstallation::User) => "maintenance.flatpak_user",
            Self::Flatpak(FlatpakInstallation::System) => "maintenance.flatpak_system",
        })
    }
    pub fn hint(&self) -> String {
        t(match self {
            Self::Orphans => "maintenance.orphans_hint",
            Self::Cache => "maintenance.cache_hint",
            Self::Flatpak(_) => "maintenance.flatpak_hint",
        })
    }
}
impl Operation {
    fn lines(&self) -> Vec<String> {
        match self {
            Self::Orphans(items) => items
                .iter()
                .map(|p| format!("{} {} · {}", p.name, p.version, size(p.size)))
                .collect(),
            Self::Cache(plan) => plan
                .files
                .iter()
                .map(|f| format!("{} · {}", f.path.display(), size(Some(f.size))))
                .collect(),
            Self::Flatpak(_, refs) => refs
                .iter()
                .map(|r| format!("{} · {}", r.reference, size(Some(r.size))))
                .collect(),
            Self::Permissions(change) => {
                if change.reset {
                    vec![t("permissions.reset_hint")]
                } else {
                    change
                        .changes
                        .iter()
                        .map(|(p, allow)| {
                            format!(
                                "{}: {}",
                                t(p.key()),
                                t(if *allow {
                                    "permissions.allow"
                                } else {
                                    "permissions.deny"
                                })
                            )
                        })
                        .collect()
                }
            }
        }
    }
    fn bytes(&self) -> Option<u64> {
        match self {
            Self::Orphans(items) => items.iter().map(|p| p.size).sum(),
            Self::Cache(plan) => Some(plan.files.iter().map(|f| f.size).sum()),
            Self::Flatpak(_, refs) => Some(refs.iter().map(|r| r.size).sum()),
            Self::Permissions(_) => None,
        }
    }
}
fn size(value: Option<u64>) -> String {
    value
        .map(|n| format!("{:.1} MiB", n as f64 / 1048576.0))
        .unwrap_or_else(|| t("inventory.size_unknown"))
}
fn text_panel(value: &str) -> gtk4::ScrolledWindow {
    let text = gtk4::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk4::WrapMode::WordChar)
        .build();
    text.buffer().set_text(value);
    gtk4::ScrolledWindow::builder()
        .child(&text)
        .min_content_height(200)
        .max_content_height(360)
        .propagate_natural_height(true)
        .build()
}
fn execute(bridge: &UiBridge, title: String, operation: Operation) {
    bridge.package_started(&title);
    let progress = bridge.clone();
    let done = bridge.clone();
    bridge.store.management_apply_async(
        title,
        operation,
        move |line| progress.append_progress(line),
        move |result| {
            done.package_finished();
            match result {
                Ok(()) => {
                    done.toast_msg(&t("management.done"));
                    (done.reload)();
                }
                Err(error) => crate::diagnostics::present_error(&done.window, &error.to_string()),
            }
        },
    );
}
pub fn present(bridge: &UiBridge) {
    let dialog = libadwaita::AlertDialog::builder()
        .heading(t("maintenance.title"))
        .body(t("maintenance.hint"))
        .build();
    dialog.add_response("close", &t("action.close"));
    let choices = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    for kind in [
        CleanupKind::Orphans,
        CleanupKind::Cache,
        CleanupKind::Flatpak(FlatpakInstallation::User),
        CleanupKind::Flatpak(FlatpakInstallation::System),
    ] {
        let button = gtk4::Button::with_label(&kind.title());
        button.set_sensitive(match kind {
            CleanupKind::Flatpak(_) => bridge.store.config().enable_flatpak,
            _ => bridge.store.config().enable_pacman,
        });
        let bridge = bridge.clone();
        button.connect_clicked(move |_| cleanup_preview(&bridge, kind.clone()));
        choices.append(&button);
    }
    dialog.set_extra_child(Some(&choices));
    dialog.present(Some(&bridge.window));
}
fn cleanup_preview(bridge: &UiBridge, kind: CleanupKind) {
    let title = kind.title();
    let dialog = libadwaita::AlertDialog::builder()
        .heading(&title)
        .body(t("preview.loading"))
        .build();
    dialog.add_response("cancel", &t("action.cancel"));
    dialog.add_response("apply", &t("maintenance.clean"));
    dialog.set_response_enabled("apply", false);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog.set_response_appearance("apply", libadwaita::ResponseAppearance::Destructive);
    let operation = std::rc::Rc::new(std::cell::RefCell::new(None));
    let apply = operation.clone();
    let bridge_apply = bridge.clone();
    dialog.connect_response(None, move |_, response| {
        if response == "apply" {
            if let Some(op) = apply.borrow_mut().take() {
                execute(&bridge_apply, title.clone(), op);
            }
        }
    });
    let weak = dialog.downgrade();
    let hint = kind.hint();
    bridge.store.cleanup_preview_async(kind, move |result| {
        let Some(dialog) = weak.upgrade() else {
            return;
        };
        match result {
            Ok(op) => {
                let lines = op.lines();
                dialog.set_body(&format!(
                    "{hint}\n\n{}: {}\n{}: {}",
                    t("maintenance.items"),
                    lines.len(),
                    t("maintenance.estimated"),
                    size(op.bytes())
                ));
                if lines.is_empty() {
                    dialog.set_extra_child(Some(&gtk4::Label::new(Some(&t("maintenance.empty")))));
                } else {
                    dialog.set_extra_child(Some(&text_panel(&lines.join("\n"))));
                    dialog.set_response_enabled("apply", true);
                    *operation.borrow_mut() = Some(op);
                }
            }
            Err(error) => dialog.set_body(&format!(
                "{}\n\n{error}",
                tcms_core::diagnostics::explanation(&error.to_string())
            )),
        }
    });
    dialog.present(Some(&bridge.window));
}
pub fn permissions(bridge: &UiBridge, id: PackageId) {
    let dialog = libadwaita::AlertDialog::builder()
        .heading(t("permissions.title"))
        .body(t("preview.loading"))
        .build();
    dialog.add_response("close", &t("action.close"));
    let weak = dialog.downgrade();
    let editor_bridge = bridge.clone();
    bridge.store.permissions_async(id.clone(), move |result| {
        let Some(dialog) = weak.upgrade() else {
            return;
        };
        let snapshot = match result {
            Ok(s) => s,
            Err(e) => {
                dialog.set_body(&e.to_string());
                return;
            }
        };
        dialog.set_body(&format!(
            "{}\n\n{}",
            id.display_ref(),
            t("permissions.hint")
        ));
        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        let state = format!(
            "{}\n{}\n\n{}\n{}",
            t("permissions.current"),
            snapshot.effective,
            t("permissions.overrides"),
            if snapshot.overrides.is_empty() {
                t("permissions.none")
            } else {
                snapshot.overrides.clone()
            }
        );
        let expander = gtk4::Expander::builder()
            .label(t("permissions.current"))
            .child(&text_panel(&state))
            .build();
        content.append(&expander);
        let rows = gtk4::ListBox::builder()
            .selection_mode(gtk4::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        let mut controls = Vec::new();
        for permission in tcms_flatpak::permissions::Permission::ALL {
            let row = libadwaita::ActionRow::builder()
                .use_markup(false)
                .title(t(permission.key()))
                .build();
            let control = gtk4::DropDown::from_strings(&[
                &t("permissions.keep"),
                &t("permissions.allow"),
                &t("permissions.deny"),
            ]);
            control.set_valign(gtk4::Align::Center);
            control.update_property(&[gtk4::accessible::Property::Label(&t(permission.key()))]);
            row.add_suffix(&control);
            rows.append(&row);
            controls.push((permission, control));
        }
        content.append(
            &gtk4::ScrolledWindow::builder()
                .child(&rows)
                .min_content_height(240)
                .max_content_height(350)
                .propagate_natural_height(true)
                .build(),
        );
        let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        let apply = gtk4::Button::with_label(&t("permissions.review"));
        let reset = gtk4::Button::with_label(&t("permissions.reset"));
        buttons.append(&apply);
        buttons.append(&reset);
        content.append(&buttons);
        let change = tcms_flatpak::permissions::Change {
            id,
            expected_overrides: snapshot.overrides,
            changes: Vec::new(),
            reset: false,
        };
        let bridge = editor_bridge.clone();
        let apply_change = change.clone();
        let editor = dialog.downgrade();
        apply.connect_clicked(move |_| {
            let mut change = apply_change.clone();
            change.changes = controls
                .iter()
                .filter_map(|(p, d)| match d.selected() {
                    1 => Some((*p, true)),
                    2 => Some((*p, false)),
                    _ => None,
                })
                .collect();
            if change.changes.is_empty() {
                bridge.toast_msg(&t("permissions.unchanged"));
                return;
            }
            confirm_permissions(&bridge, change, editor.clone());
        });
        let bridge = editor_bridge.clone();
        let editor = dialog.downgrade();
        reset.connect_clicked(move |_| {
            let mut change = change.clone();
            change.reset = true;
            confirm_permissions(&bridge, change, editor.clone());
        });
        dialog.set_extra_child(Some(&content));
    });
    dialog.present(Some(&bridge.window));
}
fn confirm_permissions(
    bridge: &UiBridge,
    change: tcms_flatpak::permissions::Change,
    editor: glib::WeakRef<libadwaita::AlertDialog>,
) {
    let operation = Operation::Permissions(change.clone());
    let title = format!("{} · {}", t("permissions.title"), change.id.id);
    let dialog = libadwaita::AlertDialog::builder()
        .heading(&title)
        .body(format!(
            "{}\n\n{}\n\n{}",
            change.id.display_ref(),
            operation.lines().join("\n"),
            t("permissions.restart")
        ))
        .build();
    dialog.add_response("cancel", &t("action.cancel"));
    dialog.add_response("apply", &t("permissions.apply"));
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    let apply = bridge.clone();
    dialog.connect_response(None, move |_, response| {
        if response == "apply" {
            if let Some(editor) = editor.upgrade() {
                editor.close();
            }
            execute(&apply, title.clone(), operation.clone());
        }
    });
    dialog.present(Some(&bridge.window));
}
