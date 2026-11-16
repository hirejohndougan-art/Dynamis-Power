# Dynamis Power — instructions for Claude

Dynamis Power is a Linux power manager that measures the laptop it runs on and tunes itself to it. It starts from safe standard settings, tests one small change at a time, and keeps only changes that measurably help. Licence: MIT.

The project owner is a developer experienced in Electron and React who is newer to Rust. Explain Rust-specific choices briefly when they are not obvious.

## Status

Phase 1 in progress: read-only sensors, hardware inventory and the `dynamis` CLI (`inventory`, `status`, `log`, `report`, `gauge`, `hardware-report`) are in `daemon/`. Remaining for the phase 1 gate: run docs/TESTING-ON-HARDWARE.md on real laptops and fix what it finds.

Phase 2 (fixture-tested only): setting plugins in `dynamis-core/src/settings/`, the change engine in `engine.rs` (journal, baseline, trials, crash recovery, safe mode, lock), the only write path in `writer.rs`, and CLI commands `settings`, `set`, `restore`, `journal`, `unblock`. `dynamisd` (in `daemon/crates/dynamisd/`) serves the standard power-profiles D-Bus interface with three profiles from `dynamis-core/src/profiles.rs`; install files in `daemon/dist/`. Also built: health monitor for trials (`health.rs`), signed quirk list and kill switch (`quirks.rs`, `dynamis-sync` helper, `dynamis-quirktool` for maintainers), safe mode after three unclean shutdowns (`boots.rs`), journal rotation. Phase 2 is code-complete; its gate needs real-hardware testing. Phase 3 built: auto mode (`dynamis-core/src/auto.rs` rules, `context.rs` system readings, auto thread in `dynamisd`, `net.dynamis.Power1` interface, `dynamis auto`), session agent `dynamis-agent`, scenario gate in `tests/scenarios/`. Phase 4 built: calibration (`dynamis-core/src/calibrate.rs`, `display.rs`, `dynamisd/src/learn.rs`), `dynamis learn`/`learned`; its gate needs real hardware. Not yet: phase 5 (tuning interacting CPU settings), phases 6–8, the app. See `ROADMAP.md`, `daemon/README.md` and the "What is implemented today" section of `docs/SAFETY.md`.

Build and check: `cd daemon && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`.

## Stack

- `daemon/` — Rust system service (root). Sensors, setting plugins, fast loop, learner, journal. D-Bus API.
- `gui/` — Tauri + React app. Talks to the daemon only over D-Bus. Opens on demand.
- `schema/` — versioned JSON Schemas for shared results. Never change a published version; add a new one.
- `tests/fixtures/` — simulated sysfs/procfs trees used by tests.
- `docs/` — all documentation; start at `docs/README.md`. Update docs in the same change as the code.

## Safety rules (non-negotiable)

Read `docs/SAFETY.md` before writing any code that changes a system setting. In short:

1. Probe before writing: only touch interfaces that exist, within the driver's own min/max.
2. Every setting plugin implements probe, read, apply, revert, and declares a risk tier.
3. All changes are runtime-only and journaled before they are applied. Nothing is written to persistent system config.
4. One change at a time. Trial settings revert automatically (dead-man timer) unless the system stays healthy.
5. Never touch: voltage or undervolting, overclocking, firmware or BIOS, bootloader, thermal trip points, fan speed below firmware minimum, filesystem barriers, battery charge protection.
6. The daemon has no network access. Downloads (quirk list, shared summaries) go through a separate unprivileged helper and are signature-checked.
7. No language model in the control loop.
8. Never run tests against the real `/sys` or `/proc`. Tests use fixtures under `tests/fixtures/`.

## Interface rules

Full specification: `docs/UI.md`. The app must be usable by someone who has never opened a terminal.

- Plain language on main screens. No sysfs paths, kernel terms or acronyms unless behind a "Details" toggle.
- Every automatic change shows a one-line reason, its measured effect, and an Undo button.
- Nothing risky without an explicit yes. "Restore original settings" always works.
- Few notifications, never during calls or full-screen video.
- Follow the desktop's light/dark theme. Keyboard navigable, screen-reader labels, readable contrast.
- All user-facing strings go through the translation layer from day one.
- The GUI must cost nothing when closed.

## Code conventions

- Write all code fresh. Do not copy code from TLP, TuneD, auto-cpufreq or power-profiles-daemon (all GPL). Implementing the same public D-Bus interface is fine.
- Rust: `cargo fmt`, `cargo clippy -- -D warnings`, no `unsafe` without a comment explaining why.
- Every persisted format (learned settings, journal, shared results) carries a version number and a migration path.
- Keep the daemon small: target under 20 MB memory and under 0.5% average CPU, with no periodic wakeups when idle.

## Test hardware

Primary test laptop: ASUS VivoBook E410MA — Celeron N4020 (2 cores), 4 GB RAM, 58 GB eMMC, Intel UHD 600, fanless, KDE Plasma 6 on Wayland. It has unexplained hard freezes; do not trust experiment results from it until those are resolved or explained.

## Commits

Small, focused commits with a clear subject line. One logical change per pull request.
