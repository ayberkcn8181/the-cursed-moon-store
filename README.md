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
- View and search the full installed inventory, including local packages, verified AUR packages and Flatpak runtimes.
- Preview affected packages, dependencies and estimated download/disk sizes before changes.
- Manage updates for user and system Flatpaks, with per-source results and optional cache-only downloads.
- Inspect transaction history, copy logs and cancel queued operations.
- Configure package sources, source priorities and AUR helpers.
- Install and manage Proton-GE, Wine-GE and DXVK, with Steam, Lutris and Heroic discovery.
- Use the interface in nine languages, including English and Turkish.

## Requirements

- An up-to-date Arch-based system; prebuilt packages target **x86_64**.
- GTK 4.14+ and libadwaita 1.6+.
- A desktop session with a Polkit authentication agent for system package operations.
- Optional: a configured helper such as `paru` or `yay` for building and installing AUR packages.

The Arch package declares its runtime dependencies, which pacman resolves during installation.

## Installation

### Download a release

Download the **`.pkg.tar.zst`** asset and **`SHA256SUMS`** from [GitHub Releases](https://github.com/ayberkcn8181/the-cursed-moon-store/releases/latest) into a new directory. From that directory, run:

```bash
sha256sum --check --ignore-missing SHA256SUMS &&
sudo pacman -U ./the-cursed-moon-store-*.pkg.tar.zst
```

GitHub's **Source code (zip)** and **Source code (tar.gz)** downloads contain source files for building the project.

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

AUR previews show declared dependencies; the helper resolves the final build plan. Sizes that cannot be determined are shown as unknown. Plans may change if repository data changes before execution.

Settings are stored in `$XDG_CONFIG_HOME/the-cursed-moon-store/config.toml`, defaulting to `~/.config/the-cursed-moon-store/config.toml`. Check the installed version with `the-cursed-moon-store --version`.

## Contributing

Bug reports and pull requests are welcome. Include the application version, distribution, steps to reproduce and relevant logs when reporting an issue.

From a checkout with the build dependencies installed:

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

For package maintenance, AUR submission and migration from a manual installation, see the [release guide](docs/RELEASING.md) (Turkish). The development package recipe is available in [packaging/aur](packaging/aur/the-cursed-moon-store-git/PKGBUILD).

## License

[GNU General Public License v3.0 or later](LICENSE).
