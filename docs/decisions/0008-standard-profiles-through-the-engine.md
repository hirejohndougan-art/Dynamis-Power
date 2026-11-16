# 0008. Standard profiles go through the change engine

- **Status:** Accepted
- **Date:** 2026-11-16

## Context

Phase 2 needs the desktop's power slider to work (decision 0004). The three standard profiles must change real settings, and other power services usually write those settings directly.

## Decision

- A profile is a short list of setting values chosen from what the machine offers: the firmware power mode (`platform.profile`) and the CPU speed preference (`cpu.epp`) for now. See `daemon/crates/dynamis-core/src/profiles.rs`.
- Profiles are applied **one setting at a time through `Engine`**, exactly like `dynamis set`. They get the same probe checks, journal entries, read-back verification, blocking and restore.
- Balanced is always offered. Power-saver and performance are offered only when they would change something on this machine.
- The daemon claims both bus names before applying anything, and refuses to start if another service owns them. It never takes a name over or lets one be taken.
- Who may switch or hold profiles is decided by our own polkit actions (`net.dynamis.power.switch-profile`, `net.dynamis.power.hold-profile`), not power-profiles-daemon's, so the policy still exists after that package is removed. If polkit cannot be asked, the answer is no.
- The engine lock is a kernel file lock (`flock`), released automatically when a process exits, so the daemon and `dynamis` on the command line take turns safely.

## Why

One write path means one place to audit and one journal to read when something goes wrong. A profile switch is just a few ordinary changes, so `dynamis restore` undoes it like anything else.

## Consequences

- A blocked setting (after a freeze during a trial, say) is skipped by every profile until it is unblocked. The profile is still applied as far as possible and the gap is logged.
- Switching profile costs a few small file reads and writes plus one journal line per changed setting. Settings already at the right value are not written or journaled.
- The daemon's D-Bus handlers do blocking file work. That takes milliseconds, plus up to 2 seconds if the command line holds the lock; acceptable for a slider.
- Learned settings (phase 4 and later) will need their own decision on how they combine with these profiles.
