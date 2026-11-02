use gtk4::prelude::*;
use libadwaita::prelude::*;
use tcms_core::t;

fn report_view(window: &gtk4::Window, body: &str) -> (gtk4::TextBuffer, gtk4::Button) {
    let dialog = libadwaita::AlertDialog::builder()
        .heading(t("diagnostics.title"))
        .body(body)
        .build();
    dialog.add_response("close", &t("action.close"));
    let host = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
    let buffer = gtk4::TextBuffer::new(None);
    let view = gtk4::TextView::builder()
        .buffer(&buffer)
        .editable(true)
        .monospace(true)
        .wrap_mode(gtk4::WrapMode::WordChar)
        .build();
    host.append(
        &gtk4::ScrolledWindow::builder()
            .child(&view)
            .min_content_height(260)
            .max_content_height(480)
            .propagate_natural_height(true)
            .build(),
    );
    host.append(
        &gtk4::Label::builder()
            .label(t("diagnostics.privacy"))
            .wrap(true)
            .xalign(0.0)
            .build(),
    );
    let copy = gtk4::Button::with_label(&t("diagnostics.copy"));
    let buf = buffer.clone();
    copy.connect_clicked(move |button| {
        button
            .clipboard()
            .set_text(&buf.text(&buf.start_iter(), &buf.end_iter(), true))
    });
    host.append(&copy);
    dialog.set_extra_child(Some(&host));
    dialog.present(Some(window));
    (buffer, copy)
}
pub fn present_error(window: &gtk4::Window, error: &str) {
    let (buffer, _) = report_view(window, &tcms_core::diagnostics::explanation(error));
    buffer.set_text(&tcms_core::diagnostics::redact_local(&format!(
        "The Cursed Moon Store {}\n\n{error}",
        env!("CARGO_PKG_VERSION")
    )));
}
pub fn present(window: &gtk4::Window, store: &crate::store::StoreService) {
    let (buffer, copy) = report_view(window, &t("diagnostics.readonly"));
    buffer.set_text(&t("diagnostics.loading"));
    copy.set_sensitive(false);
    store.diagnostics_async(move |report| {
        buffer.set_text(&report);
        copy.set_sensitive(true);
    });
}
