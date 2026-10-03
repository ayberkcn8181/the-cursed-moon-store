#!/usr/bin/env bash
set -euo pipefail
trap 'echo "AppImage build failed at line $LINENO" >&2' ERR
cd "$(dirname "$0")/.."

if [[ $(uname -m) != x86_64 || $(id -u) == 0 ]]; then
  echo 'Build as a regular user on an up-to-date x86_64 Arch system.' >&2
  exit 1
fi
binary=$(realpath "${1:-/usr/bin/the-cursed-moon-store}")
version=$(python -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["workspace"]["package"]["version"])')
test "$("$binary" --version)" = "The Cursed Moon Store $version"
mkdir -p dist
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
appdir="$work/AppDir"
mkdir -p "$appdir/usr/lib/gio/modules" "$appdir/usr/lib/gtk-4.0"

# Pin every executable download, including the runtime embedded in the output.
download() {
  curl --fail --location --retry 3 "$1" --output "$work/$2"
  printf '%s  %s\n' "$3" "$work/$2" | sha256sum --check -
  chmod +x "$work/$2"
}
download https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20251107-1/linuxdeploy-x86_64.AppImage linuxdeploy.AppImage c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d
download https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage appimagetool.AppImage ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
download https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64 runtime-x86_64 2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
export APPIMAGE_EXTRACT_AND_RUN=1

# Dynamic modules are not visible in the executable's ELF dependencies.
libraries=()
shopt -s nullglob
pixbuf_modules=(/usr/lib/gdk-pixbuf-2.0/2.10.0/loaders/*.so)
# Modern Arch uses Glycin and may ship no legacy GdkPixbuf modules.
# Bundle optional modules when present; an empty directory is valid.
for module in "${pixbuf_modules[@]}" /usr/lib/gio/modules/*.so; do
  libraries+=(--library "$module")
done
"$work/linuxdeploy.AppImage" --appdir "$appdir" \
  --executable "$binary" \
  --desktop-file data/com.cursedmoon.Store.desktop \
  --icon-file data/icons/hicolor/scalable/apps/com.cursedmoon.Store.svg \
  --custom-apprun packaging/appimage/AppRun "${libraries[@]}"

# linuxdeploy places module libraries alongside their dependencies in usr/lib.
# Relative loader names resolve through AppRun's library path after relocation.
: > "$appdir/usr/lib/loaders.cache"
if (( ${#pixbuf_modules[@]} > 0 )); then
  /usr/bin/gdk-pixbuf-query-loaders "${pixbuf_modules[@]}" | \
    sed 's|"/usr/lib/gdk-pixbuf-2.0/2.10.0/loaders/|"|g' > "$appdir/usr/lib/loaders.cache"
fi
for module in /usr/lib/gio/modules/*.so; do
  ln -s "../../$(basename "$module")" "$appdir/usr/lib/gio/modules/$(basename "$module")"
done
gio-querymodules "$appdir/usr/lib/gio/modules"

mkdir -p "$appdir/usr/share/glib-2.0" "$appdir/usr/share/icons/hicolor"
cp -a /usr/share/glib-2.0/schemas "$appdir/usr/share/glib-2.0/"
glib-compile-schemas "$appdir/usr/share/glib-2.0/schemas"
cp -a /usr/share/icons/Adwaita "$appdir/usr/share/icons/"
cp /usr/share/icons/hicolor/index.theme "$appdir/usr/share/icons/hicolor/"
install -Dm644 packaging/appimage/environment.keys "$appdir/usr/share/the-cursed-moon-store/environment.keys"
install -Dm644 data/com.cursedmoon.Store.metainfo.xml "$appdir/usr/share/metainfo/com.cursedmoon.Store.metainfo.xml"
install -Dm644 LICENSE "$appdir/usr/share/licenses/the-cursed-moon-store/LICENSE"
# Preserve dependency notices and exact versions for release provenance.
cp -a /usr/share/licenses/. "$appdir/usr/share/licenses/"
pacman -Q > "$appdir/usr/share/the-cursed-moon-store/build-packages.txt"
cp /etc/os-release "$appdir/usr/share/the-cursed-moon-store/build-os-release"
getconf GNU_LIBC_VERSION > "$appdir/usr/share/the-cursed-moon-store/glibc-baseline.txt"

ARCH=x86_64 "$work/appimagetool.AppImage" --no-appstream \
  --runtime-file "$work/runtime-x86_64" "$appdir" \
  "dist/the-cursed-moon-store-$version-x86_64.AppImage"
