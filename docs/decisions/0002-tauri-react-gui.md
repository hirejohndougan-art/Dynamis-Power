# 0002. Tauri and React for the app

- **Status:** Accepted
- **Date:** 2026-11-15

## Context

The app must be very user-friendly and must run well on 4 GB laptops. The maintainer knows React and Electron.

## Decision

Build the app with Tauri and React.

## Why

Reuses React skills; uses the system web view instead of bundling Chromium, so it needs far less memory than Electron; Rust backend matches the daemon.

## Consequences

Web views differ slightly between distributions; test on the major ones. Native desktop widgets (a Plasma applet, a GNOME extension) may still be added later for the tray.

## Alternatives considered

Electron (familiar, but too heavy for the target hardware). Qt/QML or GTK (native, but new toolkits to learn and two separate apps for KDE and GNOME).
