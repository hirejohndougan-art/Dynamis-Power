# 0004. Serve the standard power-profiles D-Bus interface

- **Status:** Accepted
- **Date:** 2026-11-15

## Context

KDE, GNOME and other tools already control power through power-profiles-daemon's D-Bus interface.

## Decision

The daemon claims org.freedesktop.UPower.PowerProfiles and the older net.hadess.PowerProfiles, and replaces power-profiles-daemon rather than running beside it.

## Why

Existing desktop sliders and apps work unchanged. TuneD and TLP ship the same kind of compatibility service, so the approach is proven.

## Consequences

Must follow that interface's behaviour closely, including profile holds. Only three standard profiles are visible to old clients; extended profiles map to the nearest one.

## Alternatives considered

A new interface only (desktops would need patches). Running alongside power-profiles-daemon (both would overwrite the same kernel settings).
