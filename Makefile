PREFIX ?= /usr
BINDIR ?= $(PREFIX)/bin
DATADIR ?= $(PREFIX)/share
APPID = com.cursedmoon.Store
CARGO_TARGET_DIR ?= $(CURDIR)/target

.PHONY: build release install uninstall test

build:
	CARGO_TARGET_DIR=$(CARGO_TARGET_DIR) cargo build -p tcms-app

release:
	CARGO_TARGET_DIR=$(CARGO_TARGET_DIR) cargo build -p tcms-app --release

test:
	CARGO_TARGET_DIR=$(CARGO_TARGET_DIR) cargo test --workspace

install: release
	install -Dm755 $(CARGO_TARGET_DIR)/release/the-cursed-moon-store $(DESTDIR)$(BINDIR)/the-cursed-moon-store
	install -Dm644 data/$(APPID).desktop $(DESTDIR)$(DATADIR)/applications/$(APPID).desktop
	install -Dm644 data/$(APPID).metainfo.xml $(DESTDIR)$(DATADIR)/metainfo/$(APPID).metainfo.xml
	install -Dm644 data/icons/hicolor/scalable/apps/$(APPID).svg $(DESTDIR)$(DATADIR)/icons/hicolor/scalable/apps/$(APPID).svg
	install -Dm644 data/polkit-1/actions/$(APPID).policy $(DESTDIR)$(DATADIR)/polkit-1/actions/$(APPID).policy

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/the-cursed-moon-store
	rm -f $(DESTDIR)$(DATADIR)/applications/$(APPID).desktop
	rm -f $(DESTDIR)$(DATADIR)/metainfo/$(APPID).metainfo.xml
	rm -f $(DESTDIR)$(DATADIR)/icons/hicolor/scalable/apps/$(APPID).svg
	rm -f $(DESTDIR)$(DATADIR)/polkit-1/actions/$(APPID).policy
