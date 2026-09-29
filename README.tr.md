# OmaRSS - Omarchy Linux İçin Yerel RSS ve Atom Akış Merkezi

[![Omarchy Verified Plugin](https://img.shields.io/badge/Omarchy-Verified_Plugin-22c55e?style=for-the-badge&logo=omarchy)](https://github.com/ozdil)
[![Buy Me A Coffee](https://img.shields.io/badge/Buy_Me_A_Coffee-Support_Development-FFDD00?style=for-the-badge&logo=buy-me-a-coffee&logoColor=black)](https://buymeacoffee.com/ozdil)

Omarchy Linux için Rust ile yazılmış ultra hafif, sıfır gecikmeli RSS ve Atom akış okuyucu ve masaüstü bildirim merkezi.

Geliştirici: Ozan Özdil (ozdil)  
Lisans: MIT  
Eklenti Kimliği: ozdil.omarss  

---

## Öne Çıkan Özellikler

- Yerel Rust Motoru (`omarss-engine`): Monotonik süre sınırları, POSIX sinyal izolasyonu ve katı bellek limitleri ile sınırlandırılmış güvenli süreç mimarisi.
- Evrensel Akış Ayrıştırıcı: HTML varlıklarını (entities) çözen, gereksiz etiketleri temizleyen RSS 2.0 ve Atom ayrıştırıcısı.
- Dinamik Durum Göstergesi: Okunmamış bülten olduğunda bar ikonu üzerinde anlık durum vurgusu.
- Yerel Quickshell Arayüzü:
  - Filtreleme: Okunmamışlar, Tümü ve Akış Yönetimi sekmeleri.
  - Yayın tarihi, başlık, özet ve okunma durumunu gösteren modern kartlar.
  - Tek tıkla tarayıcıda açma (`xdg-open`) ve otomatik okundu işaretleme.
  - Etkileşimli akış yönetimi: Yeni RSS/Atom kaynakları ekleme, aktif/pasif yapma veya kaldırma.
  - Toplu işlemler: Tümünü okundu işaretleme ve akışları canlı yenileme.

---

## Gereksinimler

- cargo ve rustc (Rust derleme zinciri)
- curl (Akış verilerini güvenli indirmek için)

---

## Kurulum ve Derleme

```bash
# Eklenti dizinine gidin
cd ~/.config/omarchy/plugins/ozdil.omarss

# Motoru derleyin
cargo build --release

# İkiliyi kurun
install -m 755 target/release/omarss-engine ./omarss-engine
install -m 755 target/release/omarss-engine ~/.local/bin/omarss-engine
```

---

## Doğrulama ve Testler

```bash
# Birim testleri çalıştırın
cargo test

# Omarchy eklenti doğrulamasını çalıştırın
omarchy plugin validate .
```
