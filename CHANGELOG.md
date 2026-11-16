# Changelog

All notable changes are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [semantic versioning](https://semver.org).

## [Unreleased]

### Added

- Guided sleep test, on the Battery screen and as `dynamis sleep-test`: with an explicit yes, the laptop sleeps 15–60 minutes with each kind of sleep it offers and wakes by itself (real-time clock alarm), then compares battery used per hour, devices that did not come back and resume errors in the kernel log. The original sleep type comes back; the one that woke cleanly and used least is recommended and applied (and kept) only after another yes. A type the laptop did not wake from properly is recorded as failed. `SleepTest`, `StartSleepTest`, `StopSleepTest`; engine `set_wake_alarm`.

- Devices: "Keep awake" per device model on the Battery screen, for a device that misbehaves asleep but is not caught by class or driver. Kept across Restore.
- Ignore a charger that reports wrongly (TLP's `TLP_PS_IGNORE`): Advanced → System, "When is the laptop plugged in?". Everything follows it (modes, learning, battery limits, device power). `power_source` in the auto preferences; shown in `dynamis stat`.

- "Charge fully once" on the Battery screen and in the panel menu: lifts the charge limit until the battery is full, the charger is unplugged or a day passes, then puts it back. Setting the limit by hand or Restore ends it; charging that follows habits waits meanwhile. `ChargeFullyOnce`, `full_charge` in `Status`.
- `dynamis stat`: a full diagnostic report like `tlp-stat` (the machine, service, battery, every setting with its original value and why it is or is not offered, devices, other power tools, the last changes). Also `Diagnostics()` and an Export option in the app.
- New settings: `storage.sata_alpm` (SATA link power; learning tests `med_power_with_dipm`; `min_power` not offered) and `cpu.min_freq` (lowest frequency, where the driver lists frequencies).

- Devices on the Battery screen: "Let idle USB devices sleep" and "Let idle built-in hardware sleep", off by default. On battery and not in Faster, every USB device and PCI function that is safe to sleep is set to sleep when idle; input devices, sound, printers, phones and cameras, modems, Bluetooth, graphics and the management engine never are. Devices plugged in later follow at the next reading; plugged in, in Faster, switched off or on Restore, each goes back to its own value. Engine `set_device_power` (journaled as `device-power`, originals in `devices.json`); `Devices`, `SetDevicePolicy`.

- Settings changed behind Dynamis Power's back are noticed: a setting it set that reads differently on two readings in a row is journaled as "changed by something else", with the likely tool (a running program such as TuneD, asusd or auto-cpufreq, a udev rule writing the same file, or a service such as PowerTOP's auto-tune), and never changed back. The second time in a day the setting is blocked and left to that tool. Shown in Activity under Problems. The demo has a "Pretend another tool changed a setting" button.

- After an unclean shutdown (freeze, crash, power cut), Home says what was happening: going to sleep, waking, a device reporting trouble (with Details), or nothing in the log; and whether Dynamis Power was changing something then or in the ten minutes before. Safe mode doubles as a test: unclean shutdowns with safe mode on are counted apart, and the notice says the cause is probably elsewhere; if none happen, switching safe mode off blocks the settings changed just before the freezes. `unclean` in `Status`; `dynamis safe-mode` explains the same. The demo has a "Pretend the laptop froze" button.

- Learning says why it waits: the service counts waiting time per reason over about the last day (plugged in, low battery, warm, busy, an app holding a mode, safe mode, another trial, dry run; rests between tests do not count), and the Learning screen shows the main one with what to do. It measures how often the battery's power reading changes and makes test blocks at least four of those periods long (the time-left estimate follows). Where a battery reports energy but not power, power is worked out from the fall in energy; where it reports neither, learning says it cannot measure. `learning.waiting` and `learning.power_update_s` in `Status`.

- Kept settings: the charge limit and start (always) and any Advanced setting with "Keep after restart" are applied again at startup and after waking from sleep, with an hour's probation after which an unclean shutdown does not hold them back. Waking also re-applies the mode, learned settings and app shares, and stops a test in progress. The service notices firmware that ignores the charge limit. `SetKept`, `kept` in `Status`.

- Modes on laptops without a speed preference or firmware power mode (such as the VivoBook's Celeron N4020): Longer battery and Faster now use turbo, the speed limit (or top frequency) and the governor; learning tunes per mode there; the Modes screen says why a mode is missing. Decision record 0016.
- New settings toward replacing TLP: `cpu.min_perf_pct`, `cpu.max_freq`, `cpu.governor`, `cpu.hwp_dynamic_boost`, `audio.controller_power`, `storage.io_scheduler`, `gpu.amd_power_level`, `battery.charge_start`, `system.mem_sleep` (risky). The writer allows only the scheduler file under `/sys/block`, only the AMD power level under `/sys/class/drm`, and only `/sys/power/mem_sleep` under `/sys/power`.

- Learning in the main menu: progress as a percentage of the tests this laptop needs (with a bar, the stage, and tests and quiet battery time left), what was kept with Forget, and the tests so far. Home shows the percentage too. Service: `learning.progress` in `Status`.

- Advanced: Profiles (what each mode sets here, standard values, export and import), Sensors (live power, processor power, deep sleep, speed, waiting, temperatures), System (versions, the service's own use, other power tools, desktop and updates), Developer (dry run, TLP/TuneD advisor, raw data, restore). Service: `Profiles`, `SetProfileOverrides`, `Sensors`, `System`, `Advise`, `SetDryRun`, `dry_run` in `Status`; engine dry-run mode (`would` entries); `advisor.rs`; cpuidle sensor. Home shows a notice while dry run is on.

- Panel icon ("tray"), from login on every desktop with panel icons: the mode at a glance (logo coloured by mode, tooltip with battery and time left), left click opens the app, and a menu to pick the mode, switch automatic choice, make the battery last (2/4/6 hours or stop), open the app, or hide the icon. Drawn by the session agent over the standard StatusNotifierItem and DBusMenu protocols, so the app can stay closed. A switch on Modes shows or hides it. The service package also installs an autostart entry for desktops that do not start user services, and a second agent in a session exits.

- Phase 7: prediction.
  - Battery forecast at your usual use, from typical power on battery by hour of the week; the usual unplug time per weekday. Kept on this computer only (`habits.json`).
  - "Last until": saves only as hard as needed (no full speed → Longer battery → also slowing background apps), one level at a time by feedback; caps app requests; ends on plug-in, at the time, or with Restore.
  - Charging that follows your routine: holds at 80% while plugged in, charges fully about 2.5 hours before the usual unplug; off by default, only where the battery has a charge limit; off restores the original limit.
  - Service: `Predict`, `SetTarget`, `SetAdaptiveCharging`; `last_until` in `Status`; client and app commands; Battery screen sections and a Home line.
  - Decision record 0015.

- Phase 5: tuning settings that affect each other.
  - Turbo boost and the speed limit are tuned together after every setting has been tested on its own: one step at a time, each step an ordinary experiment, with results tied to the other values and the power mode, and a lag limit for the whole set (`dynamis-core/src/tune.rs`).
  - Learned values apply per power mode on battery; originals return on mains. Forgetting one forgets the set.
  - Calibration records format version 2 (adds `group`); version 1 single results for these two settings are dropped and re-tested.
  - The Learning tab shows what each result was tested together with.
  - Decision record 0014.

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

- The service unit no longer sets `ProtectKernelLogs=yes`, which stopped the trial health monitor from reading the kernel log on installed systems.
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
