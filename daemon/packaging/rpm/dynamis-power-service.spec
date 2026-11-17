# Fedora / RHEL package for the Dynamis Power service. Built by
# ../build-rpm.sh from binaries it has just compiled and laid out with
# ../stage.sh (passed in as %%{stage}); not a source RPM.
%global debug_package %{nil}

Name:           dynamis-power-service
Version:        %{pkgversion}
Release:        1%{?dist}
Summary:        Power manager that learns your laptop (service and command line)
License:        MIT
URL:            https://github.com/hirejohndougan-art/Dynamis-Power
Requires:       dbus
Requires:       polkit
Requires:       systemd

%description
Dynamis Power measures the laptop it runs on and tunes its power settings to
it, keeping only changes that measurably help. This package contains the
system service (dynamisd), which serves the standard power-profiles D-Bus
interface used by desktop power sliders, the dynamis command, the session
agent and the quirk-list downloader.

Installing it switches off power-profiles-daemon or tuned-ppd if they are
running; removing it restores every original setting and switches them back
on. Every change it makes is runtime-only and can be undone.

%install
cp -a %{stage}/. %{buildroot}/
install -Dm644 %{licensefile} %{buildroot}%{_licensedir}/%{name}/LICENSE
install -Dm644 %{changelogfile} %{buildroot}%{_docdir}/%{name}/CHANGELOG.md

%files
%license %{_licensedir}/%{name}/LICENSE
%doc %{_docdir}/%{name}/CHANGELOG.md
%{_bindir}/dynamis
%dir %{_libexecdir}/dynamis-power
%{_libexecdir}/dynamis-power/dynamisd
%{_libexecdir}/dynamis-power/dynamis-sync
%{_libexecdir}/dynamis-power/dynamis-agent
%{_unitdir}/dynamisd.service
%{_unitdir}/dynamis-sync.service
%{_unitdir}/dynamis-sync.timer
%{_userunitdir}/dynamis-agent.service
%config(noreplace) %{_sysconfdir}/xdg/autostart/dynamis-agent.desktop
%dir %{_datadir}/gnome-shell/extensions/dynamis-focus@dynamis.net
%{_datadir}/gnome-shell/extensions/dynamis-focus@dynamis.net/metadata.json
%{_datadir}/gnome-shell/extensions/dynamis-focus@dynamis.net/extension.js
%{_datadir}/dbus-1/system.d/net.dynamis.power.conf
%{_datadir}/polkit-1/actions/net.dynamis.power.policy

# The same steps as the Debian package (../debian/), in RPM's terms: $1 is
# how many copies will be installed after this step (1 = first install,
# 2 = upgrade; 0 in %%preun/%%postun = removal).
%post
state=/var/lib/dynamis-power
install -d -m 700 "$state"
if [ -d /run/systemd/system ]; then
    systemctl daemon-reload || :
    systemctl reload dbus 2>/dev/null || systemctl reload dbus-broker 2>/dev/null || :
    if [ "$1" -eq 1 ]; then
        : > "$state/replaced-services"
        for svc in power-profiles-daemon tuned-ppd; do
            if systemctl is-enabled --quiet "$svc" 2>/dev/null || systemctl is-active --quiet "$svc" 2>/dev/null; then
                echo "$svc" >> "$state/replaced-services"
                systemctl disable --now "$svc" 2>/dev/null || :
                systemctl mask "$svc" 2>/dev/null || :
            fi
        done
        systemctl enable --now dynamisd.service || :
        systemctl enable --now dynamis-sync.timer || :
        systemctl --global enable dynamis-agent.service || :
    else
        systemctl try-restart dynamisd.service || :
    fi
fi

%preun
if [ "$1" -eq 0 ]; then
    if [ -d /run/systemd/system ]; then
        systemctl disable --now dynamisd.service dynamis-sync.timer 2>/dev/null || :
        systemctl --global disable dynamis-agent.service 2>/dev/null || :
    fi
    /usr/bin/dynamis restore >/dev/null 2>&1 || echo "dynamis-power: restore reported a problem; settings return to normal at the next reboot anyway" >&2
fi

%postun
state=/var/lib/dynamis-power
if [ "$1" -eq 0 ]; then
    if [ -d /run/systemd/system ]; then
        systemctl daemon-reload || :
        systemctl reload dbus 2>/dev/null || systemctl reload dbus-broker 2>/dev/null || :
        if [ -f "$state/replaced-services" ]; then
            while read -r svc; do
                [ -n "$svc" ] || continue
                systemctl unmask "$svc" 2>/dev/null || :
                systemctl enable --now "$svc" 2>/dev/null || :
            done < "$state/replaced-services"
        fi
    fi
    rm -f "$state/replaced-services"
fi
