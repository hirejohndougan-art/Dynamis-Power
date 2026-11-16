# 0010. No systemd watchdog for now; rate limits arrive with automatic changes

- **Status:** Accepted
- **Date:** 2026-11-16

## Context

SAFETY.md listed two rails that turned out to conflict with other rules or to have nothing to protect yet:

- **A systemd watchdog** restarts a service that stops sending "I'm alive" messages. Sending them means waking up on a timer, which breaks the rule of no periodic wakeups when idle.
- **A limit on changes per hour** protects against an automatic loop flapping a setting. In phase 2 every change comes from a person (a command or the power slider).

## Decision

- No watchdog. The risk it covers, a hung service leaving a change half-applied, is already covered by the engine: every change is marked pending before it is written, and the next start undoes any pending change and blocks the setting. A hung D-Bus service also shows in the desktop as a slider that does not respond, and systemd restarts it if it crashes.
- The change rate limit is added in phase 3, together with the first changes Dynamis Power makes on its own, and applies only to those.

## Consequences

A service that hangs without crashing stays hung until restarted. If that is seen in practice, revisit with a watchdog that only runs while a change is in progress.
