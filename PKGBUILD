# Maintainer: Ozan Özdil <ozan@pm.me>
pkgname=omarchy-omarss
pkgver=1.0.0
pkgrel=1
pkgdesc="Native, lightweight RSS & Atom feed reader and notification hub for Omarchy Linux written in Rust"
arch=('x86_64')
url="https://github.com/ozdil/omarchy-omarss"
license=('MIT')
depends=('glibc' 'curl')
makedepends=('cargo' 'rust')
source=("$pkgname-$pkgver.tar.gz::$url/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('SKIP')

build() {
  cd "$pkgname-$pkgver"
  cargo build --release --locked
}

package() {
  cd "$pkgname-$pkgver"
  install -Dm755 target/release/omarss-engine "$pkgdir/usr/lib/omarchy/plugins/omarss/omarss-engine"
  install -Dm755 omarss-status "$pkgdir/usr/lib/omarchy/plugins/omarss/omarss-status"
  install -Dm755 omarss-dashboard "$pkgdir/usr/lib/omarchy/plugins/omarss/omarss-dashboard"
  install -Dm644 Panel.qml "$pkgdir/usr/lib/omarchy/plugins/omarss/Panel.qml"
  install -Dm644 manifest.json "$pkgdir/usr/lib/omarchy/plugins/omarss/manifest.json"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}
