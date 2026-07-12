# Maintainer: Cursed Moon
pkgname=the-cursed-moon-store
pkgver=0.1.0
pkgrel=1
pkgdesc="GNOME Software-like store for Arch: pacman, Flatpak, and AUR"
arch=('x86_64')
url="https://github.com/ayberkcn8181/the-cursed-moon-store"
license=('GPL-3.0-or-later')
depends=('gtk4' 'libadwaita' 'pacman' 'flatpak' 'polkit')
makedepends=('cargo' 'git')
optdepends=('paru: AUR helper' 'yay: AUR helper' 'pkexec: privileged installs')
source=("$pkgname::git+file://$PWD")
sha256sums=('SKIP')

build() {
  cd "$pkgname"
  export CARGO_TARGET_DIR="$srcdir/target"
  cargo build --release -p tcms-app
}

package() {
  cd "$pkgname"
  export CARGO_TARGET_DIR="$srcdir/target"
  make DESTDIR="$pkgdir" PREFIX=/usr install
}
