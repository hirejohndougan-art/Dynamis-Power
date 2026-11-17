#!/bin/sh
# Install the service's files into a staging root, laid out for /usr, for
# the package builders (build-deb.sh, build-rpm.sh, the Arch PKGBUILD).
#
#   ./packaging/stage.sh <staging-root> [<binaries-dir>]
#
# Binaries come from target/release unless another folder is given. The
# unit files in ../dist/ name /usr/local (for the source install script),
# so they are adjusted on the way in. Distribution-specific files (licence
# and changelog locations, package scripts) are left to each builder.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
dist="$root/dist"
stage=$1
bin=${2:-"$root/target/release"}

libexec=/usr/libexec/dynamis-power
fix_paths() { sed "s#/usr/local/libexec/#$libexec/#g; s#/usr/local/bin/#/usr/bin/#g" "$1"; }

install -Dm755 "$bin/dynamis" "$stage/usr/bin/dynamis"
for b in dynamisd dynamis-sync dynamis-agent; do
    install -Dm755 "$bin/$b" "$stage$libexec/$b"
done
install -d "$stage/usr/lib/systemd/system" "$stage/usr/lib/systemd/user"
for u in dynamisd.service dynamis-sync.service dynamis-sync.timer; do
    fix_paths "$dist/$u" > "$stage/usr/lib/systemd/system/$u"
done
fix_paths "$dist/dynamis-agent.service" > "$stage/usr/lib/systemd/user/dynamis-agent.service"
chmod 644 "$stage"/usr/lib/systemd/system/* "$stage"/usr/lib/systemd/user/*
# The panel icon on desktops that do not start user services with the session.
install -d "$stage/etc/xdg/autostart"
fix_paths "$dist/dynamis-agent.desktop" > "$stage/etc/xdg/autostart/dynamis-agent.desktop"
chmod 644 "$stage/etc/xdg/autostart/dynamis-agent.desktop"
# GNOME: the extension that says which window is in front (decision 0024).
for f in metadata.json extension.js; do
    install -Dm644 "$dist/gnome-extension/dynamis-focus@dynamis.net/$f" \
        "$stage/usr/share/gnome-shell/extensions/dynamis-focus@dynamis.net/$f"
done
install -Dm644 "$dist/net.dynamis.power.conf" "$stage/usr/share/dbus-1/system.d/net.dynamis.power.conf"
install -Dm644 "$dist/net.dynamis.power.policy" "$stage/usr/share/polkit-1/actions/net.dynamis.power.policy"
