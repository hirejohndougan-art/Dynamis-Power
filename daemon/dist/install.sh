#!/bin/sh
# Install Dynamis Power from a source build. Run from the daemon/ folder after
# `cargo build --release`:   sudo ./dist/install.sh
#
# What it does (it shows this list and asks before doing anything):
#   - copies `dynamis` to /usr/local/bin and `dynamisd` to /usr/local/libexec
#   - installs the systemd unit, the D-Bus policy and the polkit actions
#   - stops and masks power-profiles-daemon / tuned-ppd if they are enabled,
#     and remembers which, so uninstall.sh can turn them back on
#   - starts dynamisd
#
# Nothing here writes a kernel setting. dynamisd applies the "balanced"
# profile when it starts; `dynamis restore` or uninstall.sh undoes it.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
bin="$here/../target/release"
state=/var/lib/dynamis-power

[ "$(id -u)" -eq 0 ] || { echo "Run with sudo: sudo $0"; exit 1; }
for f in "$bin/dynamis" "$bin/dynamisd"; do
    [ -x "$f" ] || { echo "Missing $f. Build first: cargo build --release"; exit 1; }
done

others=""
for svc in power-profiles-daemon tuned-ppd; do
    if systemctl is-enabled --quiet "$svc" 2>/dev/null || systemctl is-active --quiet "$svc" 2>/dev/null; then
        others="$others $svc"
    fi
done

echo "This will:"
echo "  - install dynamis to /usr/local/bin and dynamisd to /usr/local/libexec"
echo "  - install dynamisd.service, a D-Bus policy and polkit actions"
[ -n "$others" ] && echo "  - stop and mask:$others (uninstall.sh turns them back on)"
echo "  - start dynamisd, which applies the 'balanced' profile"
printf "Continue? [y/N] "
read -r answer
case "$answer" in y|Y|yes) ;; *) echo "Nothing changed."; exit 0 ;; esac

install -Dm755 "$bin/dynamis" /usr/local/bin/dynamis
install -Dm755 "$bin/dynamisd" /usr/local/libexec/dynamisd
install -Dm644 "$here/dynamisd.service" /etc/systemd/system/dynamisd.service
install -Dm644 "$here/net.dynamis.power.conf" /etc/dbus-1/system.d/net.dynamis.power.conf
install -Dm644 "$here/net.dynamis.power.policy" /usr/share/polkit-1/actions/net.dynamis.power.policy

install -dm700 "$state"
# Remember what we turned off, for uninstall.sh (one name per line).
# shellcheck disable=SC2086 # splitting the list into words is intended
printf '%s\n' $others > "$state/replaced-services"
for svc in $others; do
    systemctl disable --now "$svc" || true
    systemctl mask "$svc"
done

# Make the bus pick up the new policy.
systemctl reload dbus 2>/dev/null || systemctl reload dbus-broker 2>/dev/null || true
systemctl daemon-reload
systemctl enable --now dynamisd

echo
echo "Installed. Check it with:  powerprofilesctl  (or the battery icon in your panel)"
echo "Logs:                      journalctl -u dynamisd"
echo "Undo all changes:          sudo dynamis restore"
echo "Remove completely:         sudo $here/uninstall.sh"
