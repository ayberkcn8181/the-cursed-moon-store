# The Cursed Moon Store

[![License: GPL-3.0](https://img.shields.io/badge/License-GPL%203.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![GTK](https://img.shields.io/badge/GTK-4%20%2B%20libadwaita-4A86CF.svg)](https://gtk.org/)
[![Platform](https://img.shields.io/badge/Platform-Arch%20%2F%20CachyOS-1793D1.svg)](https://archlinux.org/)

**Arch tabanlı dağıtımlar** için GNOME Software tarzı yazılım mağazası.

Rust + GTK4 + libadwaita ile yazılmıştır. Pacman, Flatpak/Flathub ve AUR üzerinden uygulama arama, kurma, kaldırma ve güncelleme yapar.

<p align="center">
  <img src="data/icons/hicolor/scalable/apps/com.cursedmoon.Store.svg" alt="The Cursed Moon Store" width="128">
</p>

---

## Özellikler

| | |
|---|---|
| **Keşfet** | Öne çıkan uygulamalar (Flathub), kategori chip’leri, hızlı arama |
| **Kurulu** | Yüklü paketler, kaldırma onayı |
| **Güncellemeler** | Tek tek veya hepsini güncelle |
| **Kaynaklar** | Pacman · Flatpak · AUR (öncelik sırası ayarlanabilir) |
| **Detay** | Kaynaklar, izinler, lisans, bağış, hata bildirimi, **Aç** |
| **Dil** | Türkçe, İngilizce, Rusça, Fransızca, Korece, Japonca, Çince, Portekizce, İtalyanca (+ sistem dili) |
| **Gelişmiş** | `pacman.conf`, Flatpak remote’lar, AUR helper, ham config |

---

## Gereksinimler

- Arch Linux, CachyOS veya benzeri Arch tabanlı dağıtım
- GTK 4, libadwaita
- Rust toolchain (`rustup` / `cargo`) — kaynak koddan derlemek için
- İsteğe bağlı: `flatpak`, `paru` veya `yay` (AUR)

```bash
sudo pacman -S gtk4 libadwaita base-devel rust
# Flatpak / Flathub (önerilir)
sudo pacman -S flatpak
flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
```

---

## Kaynak koddan kurulum (sisteme yükleme)

Depoyu klonlayın, derleyin ve sistem dizinlerine kurun:

```bash
git clone https://github.com/ayberkcn8181/the-cursed-moon-store.git
cd the-cursed-moon-store

# Bağımlılıklar (yukarıdaki pacman satırı)

make release
sudo make install
```

Bu komut şunları yükler:

| Dosya | Konum (`PREFIX=/usr`, varsayılan) |
|-------|--------|
| Çalıştırılabilir | `/usr/bin/the-cursed-moon-store` |
| Masaüstü girişi | `/usr/share/applications/…` |
| AppStream metainfo | `/usr/share/metainfo/…` |
| İkon (SVG) | `/usr/share/icons/hicolor/scalable/apps/…` |
| Polkit kuralı | `/usr/share/polkit-1/actions/…` |

İstersen `sudo make install PREFIX=/usr/local` ile `/usr/local` altına da kurabilirsin.

Uygulamayı menüden **The Cursed Moon Store** olarak veya terminalden açın:

```bash
the-cursed-moon-store
```

### Kaldırma

```bash
cd the-cursed-moon-store
sudo make uninstall
```

### Sadece geliştirme / deneme (sisteme kurmadan)

```bash
git clone https://github.com/ayberkcn8181/the-cursed-moon-store.git
cd the-cursed-moon-store
cargo run -p tcms-app --release
```

Yapılandırma dosyası: `~/.config/the-cursed-moon-store/config.toml`

---

## Arch paketi (PKGBUILD)

Aynı kaynak ağaçtan yerel paket:

```bash
makepkg -si
```

---

## Proje yapısı

| Crate | Rol |
|-------|-----|
| `tcms-core` | Modeller, config, i18n, `Backend` trait |
| `tcms-pacman` | Sistem deposu (pacman) |
| `tcms-flatpak` | Flatpak / Flathub |
| `tcms-aur` | AUR (`paru` / `yay`) |
| `tcms-app` | GTK arayüz — ikili adı: `the-cursed-moon-store` |

---

## Katkı

Hata bildirimi ve PR’lar memnuniyetle karşılanır:

- Issues: https://github.com/ayberkcn8181/the-cursed-moon-store/issues
- Pull requests: https://github.com/ayberkcn8181/the-cursed-moon-store/pulls

```bash
cargo test --workspace
cargo clippy --workspace --all-targets
```

---

## Lisans

[GPL-3.0-or-later](LICENSE) — özgür yazılım; paylaşabilir ve değiştirebilirsiniz.

---

<p align="center">
  <sub>Made for Arch · CachyOS · GNOME / GTK desktops</sub>
</p>
