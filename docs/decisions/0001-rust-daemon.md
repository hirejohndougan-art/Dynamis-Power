# 0001. Rust for the daemon

- **Status:** Accepted
- **Date:** 2026-11-15

## Context

The daemon runs as root and stays resident on low-end laptops. It needs memory safety, a small footprint and no garbage-collection pauses.

## Decision

Write the daemon in Rust.

## Why

Memory safety without a garbage collector; small binaries; good D-Bus (zbus) and async support; Tauri's backend is Rust too, so the project uses two languages, not three.

## Consequences

Steeper learning curve for contributors than Python or Go. Mitigated by small plugins and clear docs.

## Alternatives considered

Python (simplest; TuneD uses it, but heavier and slower to start). Go (simple and safe, but larger binaries and a garbage collector).
