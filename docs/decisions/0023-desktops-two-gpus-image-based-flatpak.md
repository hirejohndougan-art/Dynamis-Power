# 0023: Desktops, laptops with two graphics chips, image-based systems and Flatpak

**Status:** Accepted

## Context

Dynamis Power was designed for laptops on ordinary package-based distributions. Other machines and systems need a stated position.

## Decision

- **Desktop computers:** supported. Battery features (forecast, Make it last, charging, charge limit, battery-only learning) are hidden; modes, auto mode, apps and devices work. Learning on mains waits for a power measurement that does not come from a battery (processor energy counters), so it is limited there at first.
- **Laptops with two graphics chips:** switching between the chips is left to the tools built for it (switcheroo-control, supergfxctl, the vendor's own). Dynamis Power only lets the unused chip sleep (`gpu.dgpu_runtime_pm`), and not while such a tool is running.
- **Image-based systems** (Fedora Silverblue and Kinoite, and similar): the service is installed as a layered package (`rpm-ostree install`). It writes only runtime settings and its own state under `/var/lib`, which these systems allow.
- **Flatpak:** only the app can be a Flatpak. The service needs root and direct access to `/sys`, so it is always a normal package. A Flatpak app talks to it over the system bus as the packaged app does.

## Consequences

- One package format per distribution family for the service, and an optional Flatpak for the app.
- The welcome screen says which features apply to the machine (no battery, a second graphics chip managed by another tool).
