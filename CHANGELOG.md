# Changelog

All notable changes are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [semantic versioning](https://semver.org).

## [Unreleased]

### Added

- Phase 6: apps.
  - Per-app battery use (estimates: power above idle, shared by processor time) for today and the last 7 days, kept on this computer only.
  - Priority for the app in front and slowing background apps on battery, by changing only the app's share of the processor (`cpu.weight` in `user.slice`); off by default, journaled, undone by Restore. Per-app *never slow* and *always slow*.
  - Per-app modes in auto mode ("use Faster while Blender is open"); opening such an app switches at once. New scenario `tests/scenarios/app-rules.json`.
  - Session agent: the window in front on KDE Plasma, from a small KWin script; only the process ID is sent.
  - Service: `Apps`, `SetAppPolicy`, `ForgetAppUsage`; client and app commands; Apps screen; Activity shows share changes.
  - Systemd unit: the cgroup tree is read-only except `user.slice`.
  - Decision record 0013.

- App: first-run welcome (what it does, laptop check, other power tools with *Switch off*, choice of auto mode and learning; nothing changes until Start) and a Modes screen with auto mode's preferences.
- Service: `AutoPrefs`/`SetAutoPrefs` with range checks; `Conflicts`/`SwitchOffConflict` via systemd's D-Bus API, limited to known power tools, behind a new polkit action that asks for an administrator password, and recorded so package removal turns the tool back on.

- Packages: `dynamis-power-service` (service, command, agent, downloader; built by `daemon/packaging/build-deb.sh`) and `dynamis-power` (the app, which depends on it). Removing restores every original setting and turns power-profiles-daemon back on; purging deletes the journal. Lintian-clean of errors; CI builds both.

- The desktop app (Tauri + React), started ahead of phase 8:
  - Home (battery time, power mode and auto mode, live power, learning, last change with Undo, Restore original settings), Battery, Activity (plain-language history, filters, Undo, logs, export with a privacy filter and preview), Advanced (settings, learning results, safety).
  - Every string through a translation layer; plain names for setting values; light and dark; keyboard and screen-reader labels; costs nothing when closed.
  - Builds as a 2.2 MB `.deb`; CI builds it on every push.
- Service API for the app on `net.dynamis.Power1`: `Status`, `Journal`, `Settings`, `Inventory`, `Undo`, `Restore`, `SetSetting`, `SetSafeMode`; and `dynamis-client`, the Rust client the app uses.
- Engine: undo one setting (`restore_one`); `overview` module for setting summaries.

- Phase 4: learning.
  - Calibration tests one setting at a time in alternating blocks at quiet moments on battery, and keeps a change only when it saves power with confidence and adds no lag. Every result is recorded with its measured effect.
  - Learned settings apply on battery and go back to their original values on mains; `dynamis learned forget` undoes one; switching learning off undoes all.
  - Display power model: watts per step of brightness, learned passively.
  - `dynamis learn on|off|status` and `dynamis learned`; `Learning`, `LearningStatus`, `LearnedJson` and `Forget` on `net.dynamis.Power1`.
  - Tested end to end against a simulated laptop with known effects.
  - Decision record 0012.

### Fixed

- Crash recovery no longer undoes a change whose owner process is still running (needed for tests that span many steps, and for running `dynamis` commands meanwhile).
- The lag reading cannot be inflated by a pressure counter missing from one reading.

- Phase 3: auto mode.
  - Chooses the profile from what the laptop is doing: eight workload classes (idle, light and busy use, video, call, heavy work, gaming, transfers) plus charger, battery level and temperature. Plain-language reason for every decision.
  - Smoothed: hysteresis, 60 s between switches, at most 12 automatic switches an hour; urgent changes (charger, critical battery, a call, coming back) act at once. A profile picked by hand pauses auto mode until the charger changes.
  - `dynamis-agent`, a small event-driven session agent, reports media playing, screen locked, screen kept on, and games (GameMode). Nothing else.
  - `net.dynamis.Power1` D-Bus interface (auto mode, workload, reason) and `dynamis auto on|off|status`.
  - Scripted scenarios in `tests/scenarios/` as the phase 3 gate.
  - Measured: auto mode off, no wakeups; on, one reading every 15 s at unmeasurably small CPU; service 4.7 MB, agent 3.9 MB.
  - Decision record 0011.

- Phase 2 completed in code:
  - Health monitor: a trial is undone and the setting blocked the moment the kernel reports trouble, memory or disk thrashes, or the laptop overheats.
  - Quirk list: built in, plus a signed list downloaded daily by `dynamis-sync`, an unprivileged helper. A newly blocked setting that is already applied is put back (the kill switch). `dynamis quirks` shows what applies; `dynamis-quirktool` creates keys and signs lists. The first built-in quirk blocks forced PCIe power saving on Realtek RTL8821CE Wi-Fi.
  - Safe mode switches on after three unclean shutdowns in a row and stays on until `dynamis safe-mode off`.
  - The journal rotates at 1 MB.
  - Decision records 0009 and 0010.

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
