# Packaging

Two `.deb` packages, for Ubuntu, KDE neon, Debian and relatives:

| Package | Contents | Built by |
| --- | --- | --- |
| `dynamis-power-service` | `dynamisd`, the `dynamis` command, the session agent, the quirk-list downloader, their systemd units, D-Bus policy and polkit actions | `daemon/packaging/build-deb.sh` |
| `dynamis-power` | The app. Depends on `dynamis-power-service` | `cd gui && npx tauri build --bundles deb` |

CI builds both on every push; download them from the run's *Artifacts*.

## Building

```sh
cd daemon
./packaging/build-deb.sh                      # → target/deb/dynamis-power-service_<version>_<arch>.deb
DEB_MAINTAINER="Your Name <you@example.org>" ./packaging/build-deb.sh   # with a real contact
```

The package installs to `/usr` (binaries in `/usr/libexec/dynamis-power/`); the unit files in `../dist/` name `/usr/local` for the source install script, and the build adjusts them.

## What installing and removing do

| Step | Script | Does |
| --- | --- | --- |
| First install | `debian/postinst` | Creates `/var/lib/dynamis-power`; switches off and masks power-profiles-daemon or tuned-ppd if running, and records which; starts `dynamisd` and the daily quirk check; enables the session agent for every user from their next login |
| Upgrade | `debian/postinst` | Restarts `dynamisd` |
| Remove | `debian/prerm`, `debian/postrm` | Stops everything, **restores every original setting**, turns back on the service it replaced. Keeps the journal and learned settings |
| Purge | `debian/postrm` | Also deletes the journal, learned settings and downloaded quirk list |

Service steps only run when systemd is running, so the package also installs cleanly in containers and chroots.

## Checked

- `lintian`: no errors. Expected warnings: maintainer scripts call `systemctl` directly (needed to switch off another package's service), and there is no manual page yet.
- Install, remove and purge in a container; installing the app pulls in the service; the app alone is refused.
- Not yet checked on a real desktop: the systemd steps. That is part of the phase 2 hardware test.
