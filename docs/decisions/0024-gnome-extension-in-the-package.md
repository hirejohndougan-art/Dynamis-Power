# 0024: The GNOME extension ships in the package; extensions.gnome.org later

**Status:** Accepted

## Context

On GNOME, knowing which window is in front (for app priority and per-app modes) needs a small shell extension; GNOME does not tell other programs. Extensions can be shipped in a package or published on extensions.gnome.org, which reviews each version for each GNOME release.

## Decision

- Ship the extension in the app's package (installed under `/usr/share/gnome-shell/extensions/`), so it matches the installed version and needs no account or review.
- The app explains how to switch it on (GNOME leaves extensions off until the person does).
- Publish it on extensions.gnome.org once the project is released and the extension is stable, for people who install the app another way.
- Without the extension, everything except "the app in front" works.

## Consequences

- Each GNOME release may need a small update to the extension's metadata, done with Dynamis Power releases.
