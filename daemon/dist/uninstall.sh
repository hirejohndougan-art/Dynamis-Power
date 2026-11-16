#!/bin/sh
# Remove Dynamis Power and put the laptop back how it was:
#   sudo ./dist/uninstall.sh
#
# Order matters: stop the daemon first (so it cannot re-apply a profile), then
# restore every original setting, then remove files, then turn back on any
# power service install.sh replaced.
set -eu

state=/var/lib/dynamis-power
[ "$(id -u)" -eq 0 ] || { echo "Run with sudo: sudo $0"; exit 1; }

systemctl disable --now dynamisd dynamis-sync.timer 2>/dev/null || true

if [ -x /usr/local/bin/dynamis ]; then
    echo "Restoring original settings..."
    /usr/local/bin/dynamis restore || echo "WARNING: restore reported a problem; see: sudo dynamis journal"
fi

rm -f /usr/local/bin/dynamis /usr/local/libexec/dynamisd /usr/local/libexec/dynamis-sync \
      /etc/systemd/system/dynamisd.service \
      /etc/systemd/system/dynamis-sync.service /etc/systemd/system/dynamis-sync.timer \
      /etc/dbus-1/system.d/net.dynamis.power.conf \
      /usr/share/polkit-1/actions/net.dynamis.power.policy
systemctl daemon-reload
systemctl reload dbus 2>/dev/null || systemctl reload dbus-broker 2>/dev/null || true

if [ -f "$state/replaced-services" ]; then
    while read -r svc; do
        [ -n "$svc" ] || continue
        echo "Turning $svc back on..."
        systemctl unmask "$svc" || true
        systemctl enable --now "$svc" || true
    done < "$state/replaced-services"
fi

if [ -d "$state" ]; then
    printf "Delete the change journal and saved state in %s too? [y/N] " "$state"
    read -r answer
    case "$answer" in
        y|Y|yes) rm -rf "$state" /var/lib/dynamis-power-sync /var/lib/private/dynamis-power-sync; echo "Deleted." ;;
        *) rm -f "$state/replaced-services"; echo "Kept $state (safe to delete later)." ;;
    esac
fi

echo "Dynamis Power is removed."
