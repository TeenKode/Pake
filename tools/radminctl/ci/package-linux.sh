#!/usr/bin/env bash
# Package Rust CLI only, no proprietary Radmin binaries.
set -euo pipefail
cd "$(dirname "$0")/.."
version=$(sed -nE 's/^version = "([0-9.]+)"$/\1/p' Cargo.toml | head -1)
[[ -n "$version" ]] || { echo "Cargo version missing" >&2; exit 1; }
gnu=target/release/radminctl
musl=target/x86_64-unknown-linux-musl/release/radminctl
[[ -x "$gnu" && -x "$musl" ]] || { echo "Compile binaries first" >&2; exit 1; }
mkdir -p dist
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
for flavor in gnu musl; do
  case "$flavor" in gnu) bin="$gnu";; musl) bin="$musl";; esac
  mkdir -p "$tmp/$flavor"
  install -m 755 "$bin" "$tmp/$flavor/radminctl"
  install -m 644 README.md LICENSE "$tmp/$flavor/"
  tar -czf "dist/radminctl-$version-linux-amd64-$flavor.tar.gz" -C "$tmp/$flavor" .
done
deb="$tmp/deb"
mkdir -p "$deb/DEBIAN" "$deb/usr/bin" "$deb/usr/share/doc/radminctl"
install -m 755 "$musl" "$deb/usr/bin/radminctl"
install -m 644 README.md LICENSE "$deb/usr/share/doc/radminctl/"
cat > "$deb/DEBIAN/control" <<EOF
Package: radminctl
Version: $version
Architecture: amd64
Maintainer: Radminctl contributors <noreply@users.noreply.github.com>
Section: net
Priority: optional
Description: Radmin VPN Linux CLI controller and diagnostics
 Headless process management for an external community Wine/TAP backend.
 The proprietary Windows Radmin client is not included.
EOF
dpkg-deb --root-owner-group --build "$deb" "dist/radminctl_$version""_amd64.deb"
(cd dist && sha256sum ./*.tar.gz ./*.deb > SHA256SUMS && sha256sum --check SHA256SUMS)
