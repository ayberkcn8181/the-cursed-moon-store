//! Heuristics to classify packages as apps vs codecs / drivers / system software.

use crate::package::{Package, PackageId, PackageSource};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PackageKind {
    App,
    Codec,
    Driver,
    System,
}

impl Package {
    pub fn kind(&self) -> PackageKind {
        classify_package(&self.id, &self.name, &self.summary, &self.categories)
    }
}

pub fn classify_package(
    id: &PackageId,
    name: &str,
    summary: &str,
    categories: &[String],
) -> PackageKind {
    match id.source {
        PackageSource::Flatpak => classify_flatpak(&id.id),
        PackageSource::Pacman | PackageSource::Aur => {
            classify_repo_package(&id.id, name, summary, categories)
        }
    }
}

fn classify_flatpak(app_id: &str) -> PackageKind {
    let lower = app_id.to_ascii_lowercase();
    if looks_like_codec_token(&lower) {
        return PackageKind::Codec;
    }
    if is_flatpak_runtime_or_extension(&lower) {
        return PackageKind::System;
    }
    PackageKind::App
}

fn is_flatpak_runtime_or_extension(id: &str) -> bool {
    const MARKERS: &[&str] = &[
        ".platform",
        ".sdk",
        ".locale",
        ".debug",
        ".sources",
        ".docs",
        ".baseapp",
        ".compat.",
        ".extension.",
        ".openh264",
        ".ffmpeg",
        ".codecs",
        ".vaapi",
        ".gstreamer",
    ];
    MARKERS.iter().any(|m| id.contains(m))
}

fn classify_repo_package(
    pkg_id: &str,
    name: &str,
    summary: &str,
    categories: &[String],
) -> PackageKind {
    let id = pkg_id.to_ascii_lowercase();
    let hay = format!(
        "{} {} {}",
        id,
        name.to_ascii_lowercase(),
        summary.to_ascii_lowercase()
    );

    if looks_like_codec_token(&hay) || category_has(categories, &["Codec", "Codecs"]) {
        return PackageKind::Codec;
    }
    if looks_like_driver_token(&id, &hay) || category_has(categories, &["HardwareSettings"]) {
        return PackageKind::Driver;
    }
    if looks_like_system_package(&id, &hay) {
        return PackageKind::System;
    }

    // Desktop-entry categories imply a user-facing application.
    if has_app_desktop_category(categories) {
        return PackageKind::App;
    }

    PackageKind::App
}

fn category_has(categories: &[String], needles: &[&str]) -> bool {
    categories.iter().any(|c| {
        needles.iter().any(|n| {
            c.eq_ignore_ascii_case(n) || c.to_ascii_lowercase().contains(&n.to_ascii_lowercase())
        })
    })
}

fn has_app_desktop_category(categories: &[String]) -> bool {
    categories.iter().any(|c| {
        matches!(
            c.as_str(),
            "AudioVideo"
                | "Audio"
                | "Video"
                | "Development"
                | "Education"
                | "Game"
                | "Graphics"
                | "Network"
                | "Office"
                | "Science"
                | "Utility"
        )
    })
}

fn looks_like_codec_token(hay: &str) -> bool {
    const NEEDLES: &[&str] = &[
        "codec",
        "codecs",
        "ffmpeg",
        "gstreamer",
        "gst-plugin",
        "gst-libav",
        "gst-plugins",
        "openh264",
        "x264",
        "x265",
        "libavcodec",
        "multimedia-codecs",
        "vaapi",
        "vdpau",
        "libva",
    ];
    NEEDLES.iter().any(|n| hay.contains(n))
}

fn looks_like_driver_token(id: &str, hay: &str) -> bool {
    if id.starts_with("xf86-video-")
        || id.starts_with("xf86-input-")
        || id.starts_with("nvidia")
        || id.ends_with("-dkms")
        || id.contains("-firmware")
        || id == "linux-firmware"
        || id == "sof-firmware"
        || id == "intel-ucode"
        || id == "amd-ucode"
        || id.starts_with("vulkan-")
        || id == "mesa"
        || id.starts_with("mesa-")
        || id.starts_with("lib32-nvidia")
        || id.starts_with("lib32-mesa")
    {
        return true;
    }
    (hay.contains(" graphics driver")
        || hay.contains(" display driver")
        || hay.contains(" device driver"))
        && (hay.contains("driver") || hay.contains("firmware"))
}

fn looks_like_system_package(id: &str, hay: &str) -> bool {
    // Avoid treating end-user apps like libreoffice / librewolf as libraries.
    if id.starts_with("libre") || id.starts_with("librewolf") {
        return false;
    }

    if id.starts_with("lib32-")
        || id.starts_with("lib")
        || id.ends_with("-libs")
        || id.ends_with("-devel")
        || id.ends_with("-headers")
        || id.ends_with("-common")
        || id.ends_with("-data")
        || id.ends_with("-doc")
        || id.ends_with("-docs")
        || id.ends_with("-debug")
    {
        return true;
    }

    const EXACT: &[&str] = &[
        "linux",
        "linux-lts",
        "linux-zen",
        "linux-hardened",
        "glibc",
        "gcc",
        "binutils",
        "coreutils",
        "util-linux",
        "systemd",
        "dbus",
        "polkit",
        "pam",
        "shadow",
        "filesystem",
        "iana-etc",
        "tzdata",
        "ca-certificates",
        "pacman",
        "archlinux-keyring",
        "mkinitcpio",
        "grub",
        "efibootmgr",
        "cryptsetup",
        "lvm2",
        "mdadm",
        "btrfs-progs",
        "xfsprogs",
        "e2fsprogs",
        "man-db",
        "man-pages",
        "texinfo",
        "base",
        "base-devel",
        "linux-api-headers",
        "wireplumber",
    ];
    if EXACT.contains(&id) {
        return true;
    }

    const PREFIXES: &[&str] = &[
        "linux-",
        "systemd-",
        "glibc-",
        "gcc-",
        "pacman-",
        "mkinitcpio-",
        "grub-",
        "python-",
        "perl-",
        "ruby-",
        "php-",
        "nodejs-",
        "npm-",
        "haskell-",
        "ocaml-",
        "texlive-",
        "pipewire-",
        "alsa-",
        "jack-",
        "pulseaudio-",
    ];
    PREFIXES.iter().any(|p| id.starts_with(p))
        || hay.contains("kernel module")
        || hay.contains("system library")
        || hay.contains("shared libraries")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::{PackageId, PackageSource};

    #[test]
    fn classifies_common_cases() {
        let firefox = PackageId::new(PackageSource::Pacman, "firefox");
        assert_eq!(
            classify_package(&firefox, "Firefox", "Web browser", &[]),
            PackageKind::App
        );

        let gst = PackageId::new(PackageSource::Pacman, "gst-plugins-ugly");
        assert_eq!(
            classify_package(&gst, "gst-plugins-ugly", "GStreamer codecs", &[]),
            PackageKind::Codec
        );

        let nvidia = PackageId::new(PackageSource::Pacman, "nvidia");
        assert_eq!(
            classify_package(&nvidia, "nvidia", "NVIDIA drivers", &[]),
            PackageKind::Driver
        );

        let glibc = PackageId::new(PackageSource::Pacman, "glibc");
        assert_eq!(
            classify_package(&glibc, "glibc", "GNU C Library", &[]),
            PackageKind::System
        );

        let libre = PackageId::new(PackageSource::Pacman, "libreoffice-fresh");
        assert_eq!(
            classify_package(&libre, "LibreOffice", "Office suite", &[]),
            PackageKind::App
        );

        let runtime = PackageId::new(PackageSource::Flatpak, "org.freedesktop.Platform");
        assert_eq!(
            classify_package(&runtime, "Platform", "", &[]),
            PackageKind::System
        );
    }
}
