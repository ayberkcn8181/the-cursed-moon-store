#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ $(id -u) == 0 ]]; then
  echo 'Run this build as an unprivileged user.' >&2
  exit 1
fi

shellcheck -s bash -e SC2034,SC2154 PKGBUILD packaging/aur/the-cursed-moon-store-git/PKGBUILD
shellcheck scripts/ci-arch-package.sh
tcms_args=(--output dist)
if [[ -n ${TCMS_RELEASE_TAG:-} ]]; then
  tcms_args+=(--tag "$TCMS_RELEASE_TAG")
fi
python scripts/prepare-release.py "${tcms_args[@]}"
cd dist
shellcheck -s bash -e SC2034,SC2154 PKGBUILD
makepkg --printsrcinfo > .SRCINFO
# All dependencies were installed by the CI container setup. Do not run sudo
# or disable tests here. Build and check use Cargo.lock with --frozen.
makepkg --noconfirm --cleanbuild

mkdir aur
cp ../packaging/aur/the-cursed-moon-store-git/PKGBUILD aur/PKGBUILD
(
  cd aur
  makepkg --printsrcinfo > .SRCINFO
  tar -czf ../the-cursed-moon-store-git-aur.tar.gz PKGBUILD .SRCINFO
)
sha256sum -- *.pkg.tar.zst *.tar.gz PKGBUILD .SRCINFO SOURCE_COMMIT > SHA256SUMS
sha256sum --check SHA256SUMS
