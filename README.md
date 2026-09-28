# The Cursed Moon Store

[![License: GPL-3.0](https://img.shields.io/badge/License-GPL%203.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](https://www.rust-lang.org/)
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
| **Uyumluluk araçları** | Proton-GE, Wine-GE ve DXVK kurulumu; Steam, Lutris ve Heroic keşfi |

---

## Gereksinimler

- Arch Linux, CachyOS veya benzeri Arch tabanlı dağıtım
- GTK 4.14+, libadwaita 1.6+
- `pacman-contrib` — canlı paket veritabanını değiştirmeden güncelleme kontrolü
- Rust toolchain (`rustup` / `cargo`) — kaynak koddan derlemek için
- İsteğe bağlı: `flatpak`, `paru` veya `yay` (AUR)

```bash
sudo pacman -Syu gtk4 libadwaita base-devel rust pacman-contrib
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

make CARGO_TARGET_DIR="$PWD/target" release &&
sudo make CARGO_TARGET_DIR="$PWD/target" install
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

## Dağıtım: GitHub ve AUR

**GitHub Releases** için tek kurulabilir dosya `.pkg.tar.zst` biçimindedir.
`vMAJOR.MINOR.PATCH` etiketi gönderildiğinde iş akışı Arch üzerinde derler,
test eder, paketi kurarak doğrular ve başarılıysa Release'e ekler.
Main/PR derlemeleri aynı dosyaları Actions artifact'i olarak saklar.

[AUR ve sürüm yayınlama rehberi](docs/RELEASING.md), ilk yayın için gereken
adımları ve elle kurulumdan pacman paketine geçişi açıklar. AUR geliştirme
paketinin tarifi `packaging/aur/the-cursed-moon-store-git/PKGBUILD` konumundadır;
AUR'da görünmesi için ayrıca AUR hesabınızla gönderilmesi gerekir.

GitHub'dan indirilen paketi `sudo pacman -U ./DOSYA.pkg.tar.zst` ile kurabilirsiniz.
Kurulu uygulamanın sürümünü `the-cursed-moon-store --version` ile kontrol edin.

Aynı commit edilmiş kaynak ağaçtan yerel paket:

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
| `tcms-compatibility` | Proton/Wine/DXVK ve oyun başlatıcıları |
| `tcms-app` | GTK arayüz — ikili adı: `the-cursed-moon-store` |

---

## Paket işlemlerinin davranışı

- Sistem paketi kurmak veya güncellemek, onayınızdan sonra `pacman -Syu` ile
  bekleyen tüm sistem yükseltmelerini de uygular. Tek paketlik kısmi yükseltme yapılmaz.
- Yenileme/güncelleme kontrolü `checkupdates` ile ayrı bir veritabanı kullanır;
  sistemin canlı veritabanında yalnızca `-Sy` çalıştırılmaz.
- Güvenli güncelleme kontrolü sistemin `/etc/pacman.conf` dosyasını kullanır.
  Özel bir `pacman_conf` seçildiğinde desteklenmeyen kontrol açık hata verir;
  kurma/kaldırma işlemleri seçilen yapılandırmayı kullanmaya devam eder.
- Flatpak güncellemeleri `flatpak update` kullanır. Toplu işlem runtime ve
  ilgili uzantıları da kapsar.
- Paket değişiklikleri uygulama içinde sıraya alınır. Toplu güncelleme önce
  sistemi, sonra Flatpak'i, ardından AUR'u işler. Sistem yükseltmesi başarısızsa
  AUR aşaması atlanır; tamamlanan ve başarısız kaynaklar ayrı bildirilir.
- DXVK geri alma yalnızca bilinen DLL/yedek yollarına erişir. İşlemler açılmış
  prefix diziniyle sınırlıdır; değiştirilmiş durum dosyaları ve sembolik bağlantılar
  reddedilir. İşlem öncesinde oyunları ve başlatıcıları kapatın.


### Uyumluluk arşivleri

Proton/Wine/DXVK arşivleri özel bir geçici dizinde doğrulanır. Araç dizini
içindeki mevcut hedeflere giden göreli sembolik bağlantılar ve normal dosyalara
giden hard link'ler desteklenir. Dizin dışına çıkan, döngü oluşturan veya hedefi
bulunmayan bağlantılar; aygıt dosyaları, sparse dosyalar, global PAX başlıkları
ve PAX boyut geçersiz kılmaları reddedilir. GNU uzun adlar ve PAX yolları desteklenir.

İndirme sınırı 1 GiB; açılmış arşiv sınırı 8 GiB, tek dosya sınırı 4 GiB ve
girdi sınırı 100.000'dir (oluşturulan üst dizinler de sayılır). Ek başlıklar
tek başına 64 KiB, toplamda 8 MiB ile; XZ çözücüsü 256 MiB bellekle sınırlıdır.
Arşiv iki geçişte okunur: önce boyut/tür kontrolleri, sonra dosya çıkarma.
Bu işlem ek açma süresi gerektirir; açılmış tar kopyası diskte tutulmaz.

Kurulum yalnızca bütün kontroller geçtikten sonra tek bir yeniden adlandırmayla
yayımlanır. Aynı sürüm zaten varsa veya eşzamanlı başka bir kurulum önce
tamamlanırsa mevcut dizin korunur. Başarısız işlemin geçici dosyaları temizlenir.

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

### Katalog performansı ve işlem görünürlüğü

- **Kurulu** sayfası etkin kaynakların bütün paketlerini gösterir. Pacman listesi
  `pacman -Q` üzerinden alınır; masaüstü dosyası olmayan komut satırı araçları,
  kütüphaneler ve sürücüler de dahildir. İsim/ikon eklemek için kullanılan
  `pacman -Qo` başarısız olursa temel paket listesi korunur; `-Q` hataları ise
  kaynak hatası olarak gösterilir. Keşfet filtreleri Kurulu sayfasını daraltmaz.
- Flatpak kurulu listesi hem `user` hem `system` kapsamındaki uygulama ve
  runtime'ları içerir. Aynı kimliğin farklı kapsamlardaki kayıtları ayrı kalır.
  Kurulu sayfasındaki arama paket adı, kimliği ve açıklamayı eşleştirir;
  GTK yalnızca görünür satırları oluşturur. Çalıştırılabilir masaüstü girdisi
  olmayan sistem paketlerinde ve runtime'larda **Aç** düğmesi gösterilmez.
- Kurulu paketler uygulama genelinde 30 saniyelik bir önbelleği paylaşır. Eşzamanlı
  sayfa istekleri aynı taramayı kullanır; ayar değişikliği, kaynak yenileme ve
  paket işlemi tamamlandığında (hata durumunda da) önbellek geçersiz kılınır.
  Uygulama dışında yapılan değişiklikler sonraki taramada, en geç önbellek süresi
  dolduktan sonraki sorguda görülür.
- Masaüstü dosyalarının paket sahipliği 128 dosyalık gruplarla sorgulanır.
  Entegrasyon testi, 257 dosya için 3 `pacman -Qo` çağrısını doğrular.
- Kurulu AUR ve Flatpak listeleri ağ üzerinden güncelleme sorgusu yapmaz.
  Güncelleme kontrolü Güncellemeler sayfasındadır. Arama ve kurulu uygulama
  ekranları, başarısız kaynakları erişilebilen sonuçlarla birlikte gösterir.
- Flatpak kimliği uygulama/runtime türü, uygulama kimliği, mimari, dal, depo ve
  kullanıcı/sistem kapsamını taşır. Tekil işlemler kaydın kapsamını korur;
  toplu güncelleme Ayarlar'da seçilen kapsamı kullanır. Şimdilik yalnızca
  `user` ve `system` desteklenir, adlandırılmış ek kurulumlar desteklenmez.
- İkonlar ortak bağlantı havuzu ve en fazla 6 eşzamanlı indirmeyle yüklenir;
  her ikon için ayrı işletim sistemi thread'i açılmaz.
- Pencerenin altındaki **İşlem çıktısı**, paket yöneticisinin stdout/stderr
  çıktısını işlem sürerken gösterir. Görünüm son 64.000 karakterle sınırlıdır;
  yoğun çıktı altında ara parçalar atlanabilir. Yüzde tahmini veya güvenli
  olmayan işlem ortası iptal sunulmaz.

### Entegrasyon testleri

`cargo test --workspace` kontrollü komut taklitleriyle Flatpak kimliğini,
kurulu liste için ağ sorgusu yapılmamasını ve toplu Pacman sahiplik sorgularını
sınar. CI ayrıca ayrı bir Arch konteynerinde gerçek `pacman` ile küçük bir yerel
paketi geçici kök dizine kurar, backend üzerinden sürümünü okur ve kaldırır:

```bash
# Yalnızca geçici Arch CI/test konteynerinde root olarak:
TCMS_ARCH_SMOKE=1 cargo test --locked -p tcms-pacman --test arch_smoke -- --ignored
```

Bu test masaüstü Polkit oturumunu, gerçek AUR derlemesini veya Flathub ağından
kurulumu kapsamaz; bunlar canlı sistem doğrulamasının kalan parçalarıdır.

CI ayrıca Xvfb üzerinde 2.000 paketlik Kurulu listesinin satırları ihtiyaç
oldukça oluşturmasını, aramasını ve doğru paket detayını açmasını sınar.
`pacman -Qo` hatası taklit edilerek 3.000 paketin korunması ayrıca test edilir.
