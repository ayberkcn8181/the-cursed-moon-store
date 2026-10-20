# Maintainer: Cursed Moon
pkgname=the-cursed-moon-store
pkgver=0.1.2
pkgrel=1
pkgdesc="GNOME Software-like store for Arch: pacman, Flatpak, and AUR"
arch=('x86_64')
url="https://github.com/ayberkcn8181/the-cursed-moon-store"
license=('GPL-3.0-or-later')
depends=('gtk4>=4.14' 'libadwaita>=1.6' 'glib2' 'glibc' 'gcc-libs' 'cairo'
         'gdk-pixbuf2' 'pango' 'graphene' 'xz' 'pacman' 'pacman-contrib' 'flatpak' 'polkit')
makedepends=('cargo' 'git')
optdepends=('paru: AUR helper' 'yay: AUR helper')
options=('!lto' '!debug')
# Local development only; the AUR recipe is in packaging/aur/.
source=("$pkgname::git+file://$PWD")
sha256sums=('SKIP')

prepare() {
  cd "$pkgname" || return
  cargo fetch --locked
}

build() {
  cd "$pkgname" || return
  export CARGO_TARGET_DIR="$srcdir/target"
  cargo build --frozen --release -p tcms-app
}

check() {
  cd "$pkgname" || return
  export CARGO_TARGET_DIR="$srcdir/target"
  cargo test --frozen --workspace
}

package() {
  cd "$pkgname" || return
  export CARGO_TARGET_DIR="$srcdir/target"
  make DESTDIR="$pkgdir" PREFIX=/usr CARGO_TARGET_DIR="$CARGO_TARGET_DIR" install
}
