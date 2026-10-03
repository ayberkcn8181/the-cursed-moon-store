# The Cursed Moon Store

[![License: GPL-3.0-or-later](https://img.shields.io/badge/License-GPL--3.0--or--later-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](https://www.rust-lang.org/)
[![GTK](https://img.shields.io/badge/GTK-4%20%2B%20libadwaita-4A86CF.svg)](https://gtk.org/)

<p align="center">
  <img src="data/icons/hicolor/scalable/apps/com.cursedmoon.Store.svg" alt="The Cursed Moon Store" width="128">
</p>

A graphical software center for **Arch Linux and Arch-based distributions**, built with Rust, GTK4 and libadwaita. Browse, install, remove and update software from system repositories, Flatpak and the AUR in one interface.

[Download the latest release](https://github.com/ayberkcn8181/the-cursed-moon-store/releases/latest) · [Report an issue](https://github.com/ayberkcn8181/the-cursed-moon-store/issues)

## Features

- Search applications and browse featured software from Flathub.
- View and search installed system packages, Flatpak applications and runtimes.
- Manage updates with progress output and per-source results.
- Configure package sources, source priorities and AUR helpers.
- Install and manage Proton-GE, Wine-GE and DXVK, with Steam, Lutris and Heroic discovery.
- Use the interface in nine languages, including English and Turkish.

## Requirements

- An up-to-date Arch-based system; prebuilt packages target **x86_64**.
- GTK 4.14+ and libadwaita 1.6+ (included in the AppImage).
- A desktop session with a Polkit authentication agent for system package operations.
- Optional: a configured helper such as `paru` or `yay` for building and installing AUR packages.

The Arch package declares its runtime dependencies, which pacman resolves during installation.

## Installation

### Arch package

Download the **`.pkg.tar.zst`** asset and **`SHA256SUMS`** from [GitHub Releases](https://github.com/ayberkcn8181/the-cursed-moon-store/releases/latest) into a new directory. From that directory, run:

```bash
sha256sum --check --ignore-missing SHA256SUMS &&
sudo pacman -U ./the-cursed-moon-store-*.pkg.tar.zst
```

GitHub's **Source code (zip)** and **Source code (tar.gz)** downloads contain source files for building the project.

### AppImage

Download the **`.AppImage`** asset and **`SHA256SUMS`** from the same release into a new directory, then run:

```bash
sha256sum --check --ignore-missing SHA256SUMS &&
chmod +x ./the-cursed-moon-store-*-x86_64.AppImage &&
./the-cursed-moon-store-*-x86_64.AppImage
```

The AppImage bundles the graphical libraries and runs without installing the store. It targets **up-to-date Arch-based x86_64 systems** and uses the host's pacman, Flatpak, AUR helper, Polkit and Glycin image decoders. It does not add support for other distributions' package managers.

If FUSE mounting is unavailable, run the file with `--appimage-extract-and-run`. To update the store itself, close it and replace the AppImage with the new release. See [AppImage details](docs/APPIMAGE.md).

### Build from source

Install the build tools, clone the repository and build an Arch package:

```bash
sudo pacman -Syu --needed base-devel git &&
git clone https://github.com/ayberkcn8181/the-cursed-moon-store.git &&
cd the-cursed-moon-store &&
makepkg -si
```

This requires a stable Rust toolchain. `makepkg` installs missing package dependencies and runs the test suite before installation.

## Usage

Open **The Cursed Moon Store** from the application menu or run:

```bash
the-cursed-moon-store
```

Run the application as a regular user. System package installations and updates use a full pacman system upgrade; authentication is requested when needed.

Settings are stored in `$XDG_CONFIG_HOME/the-cursed-moon-store/config.toml`, defaulting to `~/.config/the-cursed-moon-store/config.toml`. Check the installed version with `the-cursed-moon-store --version`.

## Contributing

Bug reports and pull requests are welcome. Include the application version, distribution, steps to reproduce and relevant logs when reporting an issue.

From a checkout with the build dependencies installed:

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

For package maintenance and migration from a manual installation, see the [release guide](docs/RELEASING.md) (Turkish). AUR publication is currently on hold; the development recipe remains available in [packaging/aur](packaging/aur/the-cursed-moon-store-git/PKGBUILD).

## License

[GNU General Public License v3.0 or later](LICENSE).
