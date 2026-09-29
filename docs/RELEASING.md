# GitHub ve AppImage yayını

Hedef platform güncel **Arch Linux / CachyOS, x86_64**. Tek kurulabilir dosya
`.pkg.tar.zst` veya `.AppImage` biçiminde sunulur. Arch paketinin bağımlılıklarını
pacman çözer; AppImage grafik kütüphanelerini içerir ve sistem araçlarını kullanır.
Her iki biçim de güncel Arch tabanlı sistemleri hedefler. AUR yayını şimdilik
askıdadır; aşağıdaki AUR tarifleri ileride kullanılmak üzere korunur.

## GitHub Releases

1. `Cargo.toml`, çalışma alanı paketlerinin `Cargo.lock` sürümleri, yerel
   `PKGBUILD` ve AppStream metainfo sürümünü birlikte güncelleyin.
2. Değişiklikleri main'e birleştirin. **CI** ve **Packages and release**
   kontrollerinin başarılı olduğunu doğrulayın. İkinci iş akışının artifact'i,
   yayınlamadan önce indirip deneyebileceğiniz paketi içerir.
3. Temiz ve güncel main üzerinde sürüm etiketi oluşturup gönderin:

   ```bash
   git switch main && git pull --ff-only origin main &&
   git tag -a v0.1.2 -m 'The Cursed Moon Store 0.1.2' &&
   git push origin v0.1.2
   ```

`v*` etiketleri yayın iş akışını başlatır. Etiket ile Cargo/AppStream sürümleri
uyuşmazsa yayın durur. Etiket, paketlenen commit'e işaret etmelidir.
Başarılı derleme ve paket kurulum testinden sonra GitHub Release oluşur:

- `the-cursed-moon-store-0.1.2-1-x86_64.pkg.tar.zst`: kurulabilir uygulama.
- `the-cursed-moon-store-0.1.2-x86_64.AppImage`: kurulum gerektirmeyen uygulama.
- `the-cursed-moon-store-0.1.2.tar.gz`: aynı commit'in kaynak kodu.
- `PKGBUILD` ve `SRCINFO`: kaynak arşivini SHA-256 ile doğrulayan kararlı paket tarifi.
- `the-cursed-moon-store-git-aur.tar.gz`: geliştirme sürümünün AUR gönderim dosyaları.
- `SHA256SUMS` ve `SOURCE_COMMIT`: dosya bütünlüğü ve kullanılan kaynak commit'i.

İş akışı makepkg'yi yetkisiz kullanıcıyla çalıştırır, bağımlılıkları `prepare()`
aşamasında `cargo fetch --locked` ile indirir; derleme/test aşamalarında
`--frozen` kullanır. Aynı etiketi yeniden yayınlamak mevcut dosyaları ezmez;
yeni değişiklikler için yeni sürüm etiketi kullanın.

İndirilen paketin kurulumu:

```bash
# İndirdiğiniz paket ile SHA256SUMS aynı dizinde olmalı.
sha256sum --check --ignore-missing SHA256SUMS &&
sudo pacman -U ./the-cursed-moon-store-0.1.2-1-x86_64.pkg.tar.zst
the-cursed-moon-store --version
```

SHA-256 dosyası bütünlük kontrolüdür, bağımsız bir imza değildir. Uygulamayı
normal kullanıcı olarak açın; gerekli sistem işlemleri Polkit üzerinden yükselir.
GitHub'dan elle kurulan paket için yeni sürümü yine indirip `pacman -U` ile kurun;
GitHub Releases tek başına otomatik pacman güncelleme deposu oluşturmaz.

### AppImage kullanımı

Release'teki AppImage ve SHA256SUMS dosyalarını aynı boş dizine indirin:

```bash
sha256sum --check --ignore-missing SHA256SUMS &&
chmod +x ./the-cursed-moon-store-0.1.2-x86_64.AppImage &&
./the-cursed-moon-store-0.1.2-x86_64.AppImage
```

FUSE kullanılamıyorsa son komuta `--appimage-extract-and-run` ekleyin. AppImage
kendini güncellemez; yeni sürümde dosyayı değiştirin. AUR'a gönderim gerekmez.
Derleme ve uyumluluk ayrıntıları [AppImage belgesindedir](APPIMAGE.md).

### Önceki `sudo make install` kurulumundan geçiş

Pacman `/usr/bin/the-cursed-moon-store` gibi yolların zaten bulunduğunu bildirirse,
önce sahipliğini `pacman -Qo /usr/bin/the-cursed-moon-store` ile kontrol edin.
Dosyalar önceki elle kurulumunuzdan geliyorsa, aynı `PREFIX` ile o kurulumun
`sudo make uninstall` komutunu çalıştırıp paket kurulumunu tekrar deneyin.
Başka bir paketin sahip olduğu dosyaları elle silmeyin. Kullanıcı ayarları
`~/.config/the-cursed-moon-store/` içinde kalır.

## AUR: geliştirme sürümü

`packaging/aur/the-cursed-moon-store-git/PKGBUILD` GitHub'daki main'i indirir.
`pkgver()` kaynak sürümünü, commit sayısını ve kısa SHA'yı kullanır.
Paket kararlı `the-cursed-moon-store` ile çakışır ve onun yerini sağlayabilir.

AUR hesabınız ve o hesaba eklediğiniz **SSH açık anahtarı** gerekir. Paket adının
boşta olduğunu AUR'da kontrol edin. SSH özel anahtarınızı paylaşmayın.
Aşağıdakileri proje kökünde çalıştırın; AUR deposuna yalnızca paketleme dosyaları
gönderilir:

```bash
git -c init.defaultBranch=master clone \
  ssh://aur@aur.archlinux.org/the-cursed-moon-store-git.git ../tcms-aur
cp packaging/aur/the-cursed-moon-store-git/PKGBUILD ../tcms-aur/
cd ../tcms-aur
# Maintainer satırını AUR kullanıcı adınız ve tercih ettiğiniz iletişimle güncelleyin.
makepkg -s &&
makepkg --printsrcinfo > .SRCINFO &&
git add PKGBUILD .SRCINFO &&
git commit -m 'Initial package for The Cursed Moon Store' &&
git push origin master
```

`makepkg -s` paketi derler ve testleri çalıştırır; uygulamayı sisteme kurmaz.
GitHub kaynağının en güncel hali için main birleştirmesinden sonra çalıştırın.
Gönderimden sonra kullanıcılar `paru -S the-cursed-moon-store-git` veya
`yay -S the-cursed-moon-store-git` ile kurabilir. Geliştirme sürümü değişiklikleri
için yardımcı aracın `--devel` güncelleme desteği kullanılabilir.
PKGBUILD metaverisi değiştiğinde `.SRCINFO` dosyasını yeniden üretin.

## AUR: kararlı sürüm

GitHub sürümü yayımlandıktan sonra Release'e eklenen **PKGBUILD ve SRCINFO**,
`the-cursed-moon-store` adlı ayrı AUR deposuna gönderilebilir. Bu tarif main'i
takip etmez; belirtilen sürümün kaynak arşivini checksum ile doğrular.
GitHub gizli dosya adlarını değiştirebildiği için metaveri `SRCINFO` adıyla
yayımlanır; AUR deposuna kopyalarken `.SRCINFO` olarak adlandırın.
Her yeni sürümde iki dosyayı birlikte güncelleyin. `.pkg.tar.zst` ikili paketlerini
ve uygulamanın kaynak ağacını AUR Git deposuna göndermeyin.

## Yerel hazırlama

Python 3.11+ ve Git gerektirir. Kaynak değişiklikleri commit edilmiş olmalıdır:

```bash
python3 scripts/test_prepare_release.py &&
python3 scripts/prepare-release.py --output dist
cd dist
makepkg -s
```

Mevcut `dist` boş değilse araç üzerine yazmayı reddeder; farklı bir `--output`
dizini verin. Bu adım GitHub'a veya AUR'a hiçbir şey göndermez.
