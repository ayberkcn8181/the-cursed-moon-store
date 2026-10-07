use gtk4::prelude::*;
use libadwaita::prelude::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
    time::Duration,
};
use tcms_core::{
    history::{Record, Status, TransactionQueue},
    t,
};
pub fn present(window: &gtk4::Window, queue: &Arc<TransactionQueue>) {
    let dialog = libadwaita::AlertDialog::builder()
        .heading(t("history.title"))
        .build();
    dialog.add_response("close", &t("action.close"));
    let host = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    let scroll = gtk4::ScrolledWindow::builder()
        .child(&host)
        .min_content_height(300)
        .max_content_height(500)
        .propagate_natural_height(true)
        .build();
    dialog.set_extra_child(Some(&scroll));
    let active = Rc::new(Cell::new(true));
    let close = active.clone();
    dialog.connect_closed(move |_| close.set(false));
    let queue = queue.clone();
    let weak_window = window.downgrade();
    let weak_host = host.downgrade();
    let previous = Rc::new(RefCell::new(None));
    let render = move || {
        if !active.get() {
            return glib::ControlFlow::Break;
        }
        let (Some(host), Some(window)) = (weak_host.upgrade(), weak_window.upgrade()) else {
            return glib::ControlFlow::Break;
        };
        let records = queue.snapshot();
        let signature = (
            records.iter().map(|r| (r.id, r.status)).collect::<Vec<_>>(),
            queue.warning(),
        );
        if previous.borrow().as_ref() == Some(&signature) {
            return glib::ControlFlow::Continue;
        }
        *previous.borrow_mut() = Some(signature);
        while let Some(child) = host.first_child() {
            host.remove(&child);
        }
        if let Some(warning) = queue.warning() {
            host.append(
                &gtk4::Label::builder()
                    .label(format!("{}: {warning}", t("history.save_failed")))
                    .wrap(true)
                    .build(),
            );
        }
        if records.is_empty() {
            host.append(&gtk4::Label::new(Some(&t("history.empty"))));
        }
        for record in records {
            let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
            let date = glib::DateTime::from_unix_local(record.created as i64)
                .ok()
                .and_then(|d| d.format("%x %X").ok())
                .map(|d| d.to_string())
                .unwrap_or_default();
            row.append(
                &gtk4::Label::builder()
                    .label(format!(
                        "{}\n{} · {date}",
                        record.title,
                        t(record.status.key())
                    ))
                    .wrap(true)
                    .xalign(0.0)
                    .hexpand(true)
                    .build(),
            );
            if record.status == Status::Queued {
                let cancel = gtk4::Button::with_label(&t("action.cancel"));
                let q = queue.clone();
                let id = record.id;
                cancel.connect_clicked(move |_| {
                    q.cancel(id);
                });
                row.append(&cancel);
            }
            let output = gtk4::Button::with_label(&t("transaction.output"));
            let q = queue.clone();
            let w = window.downgrade();
            output.connect_clicked(move |_| {
                if let Some(w) = w.upgrade() {
                    show_output(&w, &q, &record);
                }
            });
            row.append(&output);
            host.append(&row);
        }
        glib::ControlFlow::Continue
    };
    render();
    glib::timeout_add_local(Duration::from_millis(500), render);
    dialog.present(Some(window));
}
fn show_output(window: &gtk4::Window, queue: &Arc<TransactionQueue>, record: &Record) {
    let dialog = libadwaita::AlertDialog::builder()
        .heading(&record.title)
        .build();
    dialog.add_response("close", &t("action.close"));
    let buffer = gtk4::TextBuffer::new(None);
    buffer.set_text(&record.output);
    let view = gtk4::TextView::builder()
        .buffer(&buffer)
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk4::WrapMode::WordChar)
        .build();
    let scroll = gtk4::ScrolledWindow::builder()
        .child(&view)
        .min_content_height(280)
        .max_content_height(480)
        .propagate_natural_height(true)
        .build();
    let host = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    host.append(&scroll);
    let copy = gtk4::Button::with_label(&t("history.copy"));
    let buf = buffer.clone();
    copy.connect_clicked(move |b| {
        b.clipboard()
            .set_text(&buf.text(&buf.start_iter(), &buf.end_iter(), true))
    });
    host.append(&copy);
    dialog.set_extra_child(Some(&host));
    let active = Rc::new(Cell::new(true));
    let close = active.clone();
    dialog.connect_closed(move |_| close.set(false));
    let queue = queue.clone();
    let id = record.id;
    glib::timeout_add_local(Duration::from_millis(500), move || {
        if !active.get() {
            return glib::ControlFlow::Break;
        }
        let Some(r) = queue.snapshot().into_iter().find(|r| r.id == id) else {
            return glib::ControlFlow::Break;
        };
        buffer.set_text(&r.output);
        if r.status.active() {
            glib::ControlFlow::Continue
        } else {
            glib::ControlFlow::Break
        }
    });
    dialog.present(Some(window));
}
