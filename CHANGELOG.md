# Changelog

All notable changes are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [semantic versioning](https://semver.org).

## [Unreleased]

### Added

- Phase 1: `scripts/phase1-test.sh` runs the whole real-hardware test in one command, with a PASS/FAIL checklist and one archive to attach.
- `dynamis log --sync` forces each reading to disk so a log survives a hard freeze. `report` skips a last line cut short by a freeze.

- Phase 2: the `dynamisd` system service.
  - Serves the standard power-profiles D-Bus interface under both names, so the desktop's power slider and `powerprofilesctl` work. Includes profile holds, which end when the app that made them exits.
  - Three standard profiles chosen from what each machine offers, applied through the change engine. The chosen profile is remembered across reboots.
  - Recovery at boot; refuses to start alongside another power profile service; polkit decides who may switch profiles.
  - Hardened systemd unit with no network access, D-Bus policy, polkit actions, `install.sh` and a clean `uninstall.sh`.
  - Idle cost measured on a test bus: 1.8 MB memory, one thread, no wakeups.
  - Decision record 0008.

### Changed

- The settings lock is now a kernel file lock, released automatically when a process exits, instead of a PID file. Rust 1.89 or newer is required.

- Phase 2 (started): changing settings safely, from the command line.
  - Ten setting plugins: CPU speed preference, turbo, CPU speed limit, firmware power mode, swap eagerness, disk write batching, watchdog, sound chip sleep, PCIe link power, battery charge limit.
  - Change engine: probe and limit checks, journal flushed to disk before every write, read-back verification, one change at a time, original values kept for restore, safe mode.
  - Trials that undo themselves unless confirmed, and crash-aware recovery that undoes and blocks a change left unfinished.
  - New commands: `settings`, `set`, `restore`, `journal`, `unblock`.
  - Decision record 0007: plain files for the change journal.
  - 13 new tests, all on temporary copies of the fixture.
- Phase 1 (in progress): read-only `dynamis` command with `inventory`, `status` and `log`.
  - Sensors: battery and charger, CPU frequency and turbo, pressure stall (lag), temperatures, CPU energy counters, backlight.
  - Hardware inventory with 14 tunable settings probed (read only).
  - CSV and JSON Lines logging. Log format version 2 adds the logger's own CPU and memory use.
  - `report` summarises a log; `gauge` measures the battery gauge; `hardware-report` prints a privacy-safe report for GitHub issues.
  - Real-hardware test procedure in docs/TESTING-ON-HARDWARE.md.
  - 21 tests against the synthetic laptop fixture and unit cases; Rust checks in CI.
- Project documentation: README, architecture, design, user interface, settings catalogue, measurement, sharing, safety rails, maintaining, forking, glossary and decision records.
- Contributor guides: CONTRIBUTING, code of conduct, security policy, draft privacy policy.
- Shared-results schema, version 1, with an example and automatic validation.
- Simulated hardware fixture layout for tests.
- Issue templates for bugs, hardware reports, quirk reports and feature requests.
