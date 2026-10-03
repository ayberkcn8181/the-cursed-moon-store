#!/usr/bin/env bash
set -euo pipefail
trap 'echo "AppImage smoke test failed at line $LINENO: $BASH_COMMAND" >&2' ERR
cd "$(dirname "$0")/.."
image=$(realpath "$1")
version=$(python -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["workspace"]["package"]["version"])')
actual_version=$("$image" --appimage-extract-and-run --version)
printf 'Runtime launch version: %s\n' "$actual_version"
test "$actual_version" = "The Cursed Moon Store $version"
work=$(mktemp -d --suffix=' tcms smoke')
cleanup() {
  if [[ -n ${xvfb_pid:-} ]]; then kill "$xvfb_pid" || true; fi
  rm -rf -- "$work"
}
trap cleanup EXIT
cd "$work"
"$image" --appimage-extract > /dev/null
appdir="$work/squashfs-root"
actual_version=$("$appdir/AppRun" --version)
printf 'Relocated launch version: %s\n' "$actual_version"
test "$actual_version" = "The Cursed Moon Store $version"
if ! LD_LIBRARY_PATH="$appdir/usr/lib" ldd "$appdir/usr/bin/the-cursed-moon-store" > ldd.txt 2>&1; then
  cat ldd.txt >&2
  exit 1
fi
cat ldd.txt
if grep -q 'not found' ldd.txt; then exit 1; fi
for library in libgtk-4.so libadwaita-1.so libglib-2.0.so; do
  grep -F "$appdir/usr/lib/$library" ldd.txt
done

Xvfb :98 -screen 0 1280x800x24 -nolisten tcp > xvfb.log 2>&1 &
xvfb_pid=$!
for _ in 1 2 3 4 5; do
  test -S /tmp/.X11-unix/X98 && break
  sleep 1
done
test -S /tmp/.X11-unix/X98
# Isolated settings/cache, real host pacman inventory, no privileged mutations.
mkdir -p "$work/home"
set +e
HOME="$work/home" XDG_CONFIG_HOME="$work/home/.config" XDG_CACHE_HOME="$work/home/.cache" \
  DISPLAY=:98 GDK_BACKEND=x11 GSK_RENDERER=cairo \
  dbus-run-session -- timeout 20s "$appdir/AppRun" > app.log 2>&1
status=$?
set -e
cat app.log
# A running window reaches timeout. A loader error, crash or premature exit fails.
test "$status" -eq 124
! grep -Ei 'symbol lookup error|error while loading shared libraries|Gtk-ERROR|GLib-GIO-ERROR' app.log
