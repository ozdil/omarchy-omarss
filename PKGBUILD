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

build() {
  cd "${startdir}"
  cargo build --release --locked
}

package() {
  cd "${startdir}"
  install -Dm755 target/release/omarss-engine "${pkgdir}/usr/bin/omarss-engine"
  install -Dm755 target/release/omarss-engine "${pkgdir}/usr/share/omarchy/plugins/ozdil.omarss/omarss-engine"
  install -Dm755 omarss-status "${pkgdir}/usr/share/omarchy/plugins/ozdil.omarss/omarss-status"
  install -Dm755 omarss-dashboard "${pkgdir}/usr/share/omarchy/plugins/ozdil.omarss/omarss-dashboard"
  install -Dm644 Panel.qml "${pkgdir}/usr/share/omarchy/plugins/ozdil.omarss/Panel.qml"
  install -Dm644 manifest.json "${pkgdir}/usr/share/omarchy/plugins/ozdil.omarss/manifest.json"
  install -Dm644 LICENSE "${pkgdir}/usr/share/licenses/${pkgname}/LICENSE"
}

