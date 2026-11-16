# Forking Dynamis Power

You are free to fork Dynamis Power under the [MIT License](../LICENSE). This page lists what a fork must change to run independently, so its users never get your updates mixed with ours, or ours with yours.

## Must change

| Item | Where | Why |
| --- | --- | --- |
| Project name | Package names, binaries, app name, README | Avoid confusion with the original |
| Extended D-Bus name | `org.dynamispower.*` | Two daemons must not claim the same name. Keep the standard `org.freedesktop.UPower.PowerProfiles` interface as-is |
| Quirk list URL and signing key | `dynamis-sync` config, compiled-in public key | Your fork must trust only your own quirk list |
| Submission service URL | `dynamis-sync` config | Don't send your users' results to our service, or ours to yours |
| Release signing key | Packaging | Users must be able to tell your releases from ours |
| Paths | `/var/lib/dynamis-power`, `/etc/dynamis-power`, systemd unit names | Avoid two installs clobbering each other |

These values will be kept in one place, a branding and endpoints file, so a fork changes them once. Until that file exists, search the codebase for `dynamis`.

## Can keep

- The results schema format. Using the same format makes sharing data between projects possible later.
- Setting IDs. Keeping them compatible lets quirk knowledge be shared.

## Please do

- Keep the safety rails in [SAFETY.md](SAFETY.md). Users trust anything called a power manager with their hardware.
- Credit the original project, as the MIT License requires you to keep the copyright notice.
- Consider sending fixes upstream. Quirk reports especially help everyone.
