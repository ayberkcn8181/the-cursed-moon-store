#!/usr/bin/env python3
"""Generate crates/tcms-core/src/i18n/catalog.rs"""

from pathlib import Path

EN = {
    "app.name": "The Cursed Moon Store",
    "app.developer": "Cursed Moon",
    "app.about": "About The Cursed Moon Store",
    "app.about_comments": "Software store for Arch-based distributions with pacman, Flatpak, and AUR support.",
    "nav.explore": "Explore",
    "nav.installed": "Installed",
    "nav.updates": "Updates",
    "nav.settings": "Settings",
    "nav.menu": "Menu",
    "nav.refresh": "Refresh",
    "explore.title": "Explore",
    "explore.search_placeholder": "Search pacman, Flatpak, and AUR…",
    "explore.empty_title": "Search for software",
    "explore.empty_desc": "Find packages from system repos, Flathub, and the AUR.",
    "installed.title": "Installed",
    "installed.empty_title": "No applications found",
    "installed.empty_desc": "Installed desktop apps from pacman, Flatpak, and AUR will appear here.",
    "installed.count": "{n} applications",
    "updates.title": "Updates",
    "updates.up_to_date_title": "Software is up to date",
    "updates.up_to_date_desc": "No updates are available from enabled sources.",
    "updates.count": "{n} updates available",
    "updates.update_all": "Update All",
    "updates.updating_all": "Updating all packages…",
    "updates.updated_n": "Updated {n} package(s)",
    "updates.update_all_failed": "Update all failed: {error}",
    "settings.title": "Settings",
    "settings.general": "General",
    "settings.sources": "Software Sources",
    "settings.advanced": "Advanced",
    "settings.language": "Language",
    "settings.language_desc": "Choose the interface language for The Cursed Moon Store.",
    "settings.language_restart": "Language updated. Some labels refresh immediately; reopen settings to see every change.",
    "settings.updates_group": "Updates",
    "settings.updates_group_desc": "How The Cursed Moon Store checks for new software.",
    "settings.auto_updates": "Automatic updates check",
    "settings.auto_updates_desc": "Periodically look for package updates",
    "settings.bg_download": "Download in background",
    "settings.bg_download_desc": "Fetch updates without opening the store",
    "settings.sources_group": "Enabled sources",
    "settings.sources_group_desc": "Toggle which repositories appear in Explore, Installed, and Updates.",
    "settings.source_pacman": "System repositories",
    "settings.source_pacman_desc": "Official and configured pacman repos",
    "settings.source_flatpak": "Flathub / Flatpak",
    "settings.source_flatpak_desc": "Flatpak applications and runtimes",
    "settings.source_aur": "AUR",
    "settings.source_aur_desc": "Arch User Repository (community packages)",
    "advanced.warning": "Advanced options can break package management. Edit carefully.",
    "advanced.pacman": "Pacman",
    "advanced.pacman_desc": "System repository configuration.",
    "advanced.pacman_conf": "pacman.conf path",
    "advanced.pacman_conf_desc": "Leave empty for the system default",
    "advanced.pacman_args": "Extra pacman arguments",
    "advanced.pacman_args_desc": "Example: --noconfirm --needed",
    "advanced.flatpak": "Flatpak",
    "advanced.flatpak_desc": "Installation scope and remotes (name|url per line).",
    "advanced.flatpak_install": "Installation",
    "advanced.flatpak_install_desc": "system or user",
    "advanced.flatpak_remotes": "Flatpak remotes",
    "advanced.aur": "AUR",
    "advanced.aur_desc": "RPC endpoint and build helper.",
    "advanced.aur_rpc": "RPC URL",
    "advanced.aur_rpc_desc": "AUR RPC base URL",
    "advanced.aur_helper": "Helper",
    "advanced.aur_helper_desc": "paru, yay, or makepkg",
    "advanced.aur_args": "Extra helper arguments",
    "advanced.aur_args_desc": "Passed to the AUR helper",
    "advanced.raw_group": "Raw configuration overlay",
    "advanced.raw_group_desc": "Free-form TOML merged into runtime for power users. Full store config is also editable below.",
    "advanced.allow_raw": "Allow raw config editing",
    "advanced.allow_raw_desc": "Enable the full config.toml editor",
    "advanced.raw_overlay": "Raw overlay",
    "advanced.full_config": "Full config.toml",
    "advanced.reset": "Reset Advanced to Defaults",
    "advanced.save": "Save Advanced Settings",
    "advanced.saved": "Advanced settings saved",
    "advanced.save_failed": "Failed to save settings: {error}",
    "action.install": "Install",
    "action.remove": "Remove",
    "action.update": "Update",
    "state.available": "Available",
    "state.installed": "Installed",
    "state.updatable": "Update available",
    "state.installing": "Installing",
    "state.removing": "Removing",
    "toast.installing": "Installing {name}…",
    "toast.removing": "Removing {name}…",
    "toast.updating": "Updating {name}…",
    "toast.done": "{name}: done",
    "toast.failed": "{name} failed: {error}",
    "toast.refreshing": "Refreshing software sources…",
    "toast.refreshed": "Sources refreshed",
    "package.none_title": "No software found",
    "package.none_desc": "Try another search or enable more sources in Settings.",
    "source.pacman": "System",
    "source.flatpak": "Flathub",
    "source.aur": "AUR",
}

# Load extra language overrides from sibling JSON if we embed them inline below
OVERRIDES = {}

def esc(s: str) -> str:
    return s.replace("\\", "\\\\").replace('"', '\\"')

def emit_lang(name: str, table: dict) -> list[str]:
    lines = [f"        Language::{name} => match key {{"]
    for k, v in table.items():
        lines.append(f'            "{k}" => Some("{esc(v)}"),')
    lines.append("            _ => None,")
    lines.append("        },")
    return lines

def main() -> None:
    # Import overrides from gen_i18n_data.py next to this script if present
    data_path = Path(__file__).with_name("gen_i18n_data.py")
    ns: dict = {}
    if data_path.exists():
        exec(data_path.read_text(encoding="utf-8"), ns)
        overrides = ns["OVERRIDES"]
    else:
        overrides = OVERRIDES

    langs = {"English": EN}
    for name, ov in overrides.items():
        langs[name] = {**EN, **ov}

    out = [
        "//! Translation catalogs for all supported languages.",
        "",
        "use super::Language;",
        "",
        "pub fn lookup(lang: Language, key: &str) -> Option<&'static str> {",
        "    match lang {",
    ]
    for name, table in langs.items():
        out.extend(emit_lang(name, table))
    out.extend(["    }", "}", ""])

    dest = Path("crates/tcms-core/src/i18n/catalog.rs")
    dest.write_text("\n".join(out), encoding="utf-8")
    print(f"wrote {dest} ({len(out)} lines, {len(EN)} keys, {len(langs)} languages)")

if __name__ == "__main__":
    main()
