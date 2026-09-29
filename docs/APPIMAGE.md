# AppImage

The x86_64 AppImage is an alternative download for current Arch Linux and
Arch-based distributions. It includes GTK4, libadwaita, their non-system library
dependencies, icons, GLib schemas and image loaders. It uses the host's glibc,
graphics drivers, fonts and desktop session. Builds use a fully updated Arch
container; older distributions and glibc versions are not supported.

## Running

Download the AppImage and `SHA256SUMS` from the same GitHub release into an empty
directory. Verify, make it executable and launch it as your regular user:

```bash
sha256sum --check --ignore-missing SHA256SUMS &&
chmod +x ./the-cursed-moon-store-*-x86_64.AppImage &&
./the-cursed-moon-store-*-x86_64.AppImage
```

If FUSE mounting is unavailable:

```bash
./the-cursed-moon-store-*-x86_64.AppImage --appimage-extract-and-run
```

The store still needs the host's `pacman` and `checkupdates` (`pacman-contrib`),
`flatpak` for Flatpak support, and `pkexec` (`polkit`) with an authentication agent
for system transactions. AUR installs require `paru` or `yay` and their build
dependencies. These tools and the system package database are not bundled.
Do not run the entire AppImage with sudo.

Wayland and X11 are selected by GTK using the current desktop session. The
AppImage uses the same configuration and cache as a native installation. It
does not automatically install a desktop shortcut or a system Polkit policy.
The system's normal pkexec policy handles authentication.

## Updating and removing

The Updates page manages system/Flatpak packages. The AppImage does not update
itself: close it, download and verify a new release, and replace the old file.
Deleting the AppImage removes this copy of the store; it does not remove packages
installed through the store or its user configuration.

## Building and validation

The `Packages and release` workflow builds and tests the native Arch package,
then bundles that same executable. To reproduce in an up-to-date Arch build
environment, install the dependencies listed in `.github/workflows/package.yml`,
build/install the native package, and run as a regular user from the checkout:

```bash
bash scripts/build-appimage.sh /usr/bin/the-cursed-moon-store
bash scripts/test-appimage.sh dist/*.AppImage
```

Packaging tools and the AppImage runtime are pinned and SHA-256 checked. CI
checks extraction without FUSE, relocation into a path containing spaces,
bundled GTK library resolution and a GUI startup under Xvfb. Rust tests check
that host subprocesses receive the original environment; Python tests exercise
the actual AppRun launcher. Wayland, graphics acceleration and interactive
Polkit prompts still need a real desktop test.

The bundle records build package versions, OS information and the glibc baseline
under `usr/share/the-cursed-moon-store/`, alongside dependency license notices
under `usr/share/licenses/`. AppRun saves only the variables it changes. Every
backend command and GIO application launch restores those variables for the
host process without changing the store's process-wide environment.
