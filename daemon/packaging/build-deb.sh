#!/bin/sh
# Build dynamis-power-service_<version>_<arch>.deb from this workspace.
#
#   cd daemon && ./packaging/build-deb.sh            # builds, then packages
#   ./packaging/build-deb.sh --no-build              # package existing binaries
#
# The files are laid out by stage.sh (shared with the RPM and Arch builds).
# Output goes to target/deb/.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
bin="$root/target/release"

[ "${1:-}" = "--no-build" ] || (cd "$root" && cargo build --release --locked)

version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)
arch=$(dpkg --print-architecture)
pkg=dynamis-power-service
# Set DEB_MAINTAINER="Name <email>" to put a real contact in the package.
maintainer=${DEB_MAINTAINER:-"Dynamis Power project <maintainers@dynamis-power.invalid>"}
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

"$here/stage.sh" "$stage" "$bin"
install -Dm644 "$root/../LICENSE" "$stage/usr/share/doc/$pkg/copyright"
gzip -9n -c "$root/../CHANGELOG.md" > "$stage/usr/share/doc/$pkg/CHANGELOG.md.gz"
# Debian wants its own changelog format too; point it at the real one.
printf '%s (%s) unstable; urgency=medium\n\n  * See CHANGELOG.md.gz in this folder for every change.\n\n -- %s  %s\n' \
    "$pkg" "$version" "$maintainer" "$(date -R -u -d @"${SOURCE_DATE_EPOCH:-$(git -C "$root" log -1 --format=%ct 2>/dev/null || date +%s)}")" \
    | gzip -9n > "$stage/usr/share/doc/$pkg/changelog.gz"

install -d "$stage/DEBIAN"
echo /etc/xdg/autostart/dynamis-agent.desktop > "$stage/DEBIAN/conffiles"
for s in postinst prerm postrm; do
    install -m755 "$here/debian/$s" "$stage/DEBIAN/$s"
done
size=$(du -sk --exclude=DEBIAN "$stage" | cut -f1)
cat > "$stage/DEBIAN/control" <<CONTROL
Package: $pkg
Version: $version
Architecture: $arch
Section: admin
Priority: optional
Installed-Size: $size
Depends: libc6, dbus, polkitd | policykit-1, systemd
Maintainer: $maintainer
Homepage: https://github.com/hirejohndougan-art/Dynamis-Power
Description: power manager that learns your laptop (service and command line)
 Dynamis Power measures the laptop it runs on and tunes its power settings
 to it, keeping only changes that measurably help. This package contains the
 system service (dynamisd), which serves the standard power-profiles D-Bus
 interface used by desktop power sliders, the dynamis command, the session
 agent and the quirk-list downloader.
 .
 Installing it switches off power-profiles-daemon or tuned-ppd if they are
 running; removing it restores every original setting and switches them back
 on. Every change it makes is runtime-only and can be undone.
CONTROL
(cd "$stage" && find usr -type f -exec md5sum {} + > DEBIAN/md5sums)

out="$root/target/deb"
mkdir -p "$out"
deb="$out/${pkg}_${version}_${arch}.deb"
dpkg-deb --root-owner-group -Zxz --build "$stage" "$deb" >/dev/null
echo "$deb"
