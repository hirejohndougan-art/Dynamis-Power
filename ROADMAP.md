# Roadmap

Dynamis Power is built in eight phases. Each phase is usable on its own and ends with a gate that must pass before the next one starts.

| Phase | Builds | Gate: done when |
| --- | --- | --- |
| 1. Measure **(in progress)** | Logger, hardware inventory, power report | Readings are trustworthy and the daemon stays within its overhead budget |
| 2. Profiles **(built; gate waits on hardware testing)** | Setting plugins, standard D-Bus profiles, change journal, quirk list, clean uninstall | The desktop power slider works and every change reverts |
| 3. Auto-switch **(built; scenario gate passes)** | Session agent, context rules, workload detection | The right profile is chosen in scripted scenarios |
| 4. Calibrate **(built; gate needs real hardware)** | Per-setting tests, display power model | Beats stock "Balanced" on battery |
| 5. Learn **(built; gate needs real hardware)** | Optimiser for interacting CPU settings | Beats a hand-tuned TLP configuration |
| 6. Apps **(built; gate needs real hardware)** | Per-app accounting, foreground boost, background throttle | The focused app stalls less under load |
| 7. Predict **(built; gate needs real hardware)** | "Last until" mode, adaptive charging, habits | Time targets are met within a margin |
| 8. Release | Full app, shared results, optional AI helper | Public release with published benchmark data |

Phase 2 is built: setting plugins, journal, trials with a health monitor, restore, the three standard profiles, the `dynamisd` service with the standard D-Bus interface, the signed quirk list and kill switch, safe mode after repeated unclean shutdowns, and install and clean uninstall. It was built while the phase 1 hardware test is pending, because it can be tested entirely on fixtures. Phase 2 does not count as done, and nothing from it should run on real hardware by default, until the phase 1 gate passes.

Phase 3 is built: auto mode with rules over eight workload classes, the session agent, `dynamis auto`, and the scripted scenarios in `tests/scenarios/`, which form its gate and pass. It still needs trying on a real desktop (docs/TESTING-ON-HARDWARE.md, phase 3 section).

Phase 4 is built: calibration experiments, learned settings applied on battery, the display power model, and `dynamis learn` / `dynamis learned`. On a simulated laptop it finds real savings and rejects changes that add lag. Whether it beats stock Balanced on real laptops is what the gate measures.

Phase 7 is built: a battery forecast from typical power by hour of the week, the usual unplug time, "Last until" (feedback towards a watt budget through the power mode cap and app shares), and charging that holds at 80% and charges fully before the usual unplug ([decision 0015](docs/decisions/0015-prediction-by-feedback-and-habits.md)). Its gate (time targets met within a margin) needs real days of use.

Phase 5 is built: once every setting has been tested on its own, turbo boost and the speed limit are tuned together, one step at a time, per power mode, with a lag limit for the whole set ([decision 0014](docs/decisions/0014-tuning-together-one-step-at-a-time.md)). On a simulated laptop where turbo off only helps below an 80% speed limit, it finds that combination; testing each setting once does not. Whether it beats a hand-tuned TLP setup is what the gate measures, on real laptops.

Phase 6 is built: per-app energy estimates, priority for the app in front, slowing background apps on battery, per-app "always slow" and "never slow", per-app modes in auto mode, and the Apps screen. It only ever changes an app's share of the processor, never caps or freezes it ([decision 0013](docs/decisions/0013-app-shares-not-caps.md)). Its gate (the app in front stalls less under load) needs a real laptop.

The desktop app and `.deb` packages for it and the service (part of phase 8) were started early, because it does not depend on the phase 4 result and makes phases 1–4 usable without a terminal: Home, Battery, Activity with export, and Advanced. See [gui/README.md](gui/README.md).

**Phase 4 is the go/no-go point.** If per-setting calibration cannot beat stock settings on battery, the approach gets rethought before more is built.

## To do: modes on laptops without the usual controls

Found on the VivoBook E410MA: Longer battery and Faster are hidden, so the mode choice, per-app modes, the panel icon menu and the welcome show only Balanced. A mode today changes only the processor's speed preference (EPP) and the firmware power mode (platform profile), and a mode is offered only when it would change something. Budget and older processors (Gemini Lake Celerons such as the N4020, older Intel chips, AMD and others on `acpi-cpufreq`) often have neither.

- [ ] Confirm on the VivoBook what it offers: `ls /sys/devices/system/cpu/cpufreq/policy0/`, `cat /sys/firmware/acpi/platform_profile_choices`, `cat /sys/devices/system/cpu/intel_pstate/status`. Add it as a fixture under `tests/fixtures/` (a copy of the real tree, no serial numbers)
- [ ] Fallback levers per mode, used only when EPP and the platform profile are missing, each through its own probed setting plugin: Longer battery → turbo boost off and a lower speed limit (or a lower maximum frequency where there is no speed limit); Balanced → turbo on, full speed; Faster → turbo on, full speed and the most eager governor the driver offers
- [ ] Make modes and learning agree: learning tunes turbo and the speed limit (phase 5), so on such laptops the mode sets the starting point and learning tunes on top for battery, with tuning results kept per mode (as they already are per speed preference); restoring and forgetting put back the mode's value, not a stale one
- [ ] Advanced → Profiles: show the fallback levers for each mode so they can be chosen there too
- [ ] Say why when a mode cannot be offered ("this laptop has no controls for it"), instead of hiding it silently
- [ ] Scenario and engine tests on the new fixture; then try it on the VivoBook

## To do: fully replace TLP

Goal: anything TLP manages, Dynamis Power manages too, so TLP can be removed with no loss. Each item becomes a setting plugin (probe, read, apply, revert, risk tier; see docs/SETTINGS.md) that profiles can set and learning can test, or a service behaviour. Items marked *(in SETTINGS)* already have a reserved ID there.

**Devices and buses (the biggest savings TLP has that Dynamis Power lacks)**

- [ ] USB autosuspend for idle devices, never for input devices, audio, phones, printers or mobile broadband, with a per-device "never" list *(in SETTINGS: `usb.autosuspend`)*
- [ ] Runtime power management for idle PCI devices, with a per-driver "never" list *(in SETTINGS: `pci.runtime_pm`)*
- [ ] Wi-Fi power saving on battery *(in SETTINGS: `wifi.power_save`)*; must respect the built-in quirk for the RTL8821CE and similar chips
- [ ] Wake-on-LAN off
- [ ] Sound controller power saving as well as the codec (TLP's controller option)
- [ ] Discrete and integrated GPU: runtime power-down of a second GPU *(in SETTINGS: `gpu.dgpu_runtime_pm`)*, Intel GPU frequency limits *(in SETTINGS: `gpu.max_freq`)*, AMD GPU power level

**Storage**

- [ ] SATA link power *(in SETTINGS: `storage.sata_alpm`; "minimum power" stays opt-in)*
- [ ] AHCI controller runtime power management
- [ ] Hard disk power level (APM) and spin-down time, for laptops with spinning disks
- [ ] I/O scheduler *(in SETTINGS: `storage.io_scheduler`)*

**Processor**

- [ ] Minimum performance level (`min_perf_pct`), alongside the maximum already built
- [ ] Minimum and maximum frequency for drivers without performance levels (AMD and other cpufreq drivers)
- [ ] HWP dynamic boost (Intel)
- [ ] Scaling governor where the driver offers a choice, without fighting the speed preference

**Radios**

- [ ] Switch Bluetooth, Wi-Fi or mobile broadband off on battery, at startup, or when a wired network connects, as the person chooses *(in SETTINGS: `bt.power` for Bluetooth)*; remember and restore their on/off state across restarts
- [ ] Optional rules on docking and undocking

**Battery**

- [ ] Charge start threshold (begin charging only below a level), alongside the stop threshold already built
- [ ] Vendor charge controls where the standard file is missing (ThinkPad, ASUS, Dell, Huawei, LG, Samsung, Sony, System76 and others), each probed, never guessed
- [ ] "Charge fully once" and battery recalibration on request (TLP's `fullcharge` and `recalibrate`)

**System**

- [ ] Sleep type on battery (`s2idle` or `deep`), offered only where the firmware supports both
- [ ] Re-apply settings after resume from sleep, and to devices plugged in later (USB, Thunderbolt), as TLP does
- [ ] A mode to use at boot before auto mode decides, and an option to ignore a power supply that reports wrongly (TLP's `TLP_DEFAULT_MODE` and `TLP_PS_IGNORE`)
- [ ] A full diagnostic report like `tlp-stat`, building on `dynamis status`, `inventory` and `hardware-report`
- [ ] Import a TLP configuration: read `/etc/tlp.conf` and `/etc/tlp.d/` and offer the matching Dynamis Power settings (the reverse of the advisor's TLP export)

**Done when:** on the test laptops, with TLP removed, `tlp-stat` run once with TLP reinstalled shows nothing TLP would set that Dynamis Power does not, and battery life is at least as long as with a hand-tuned TLP.

## Realistic expectations

This is a spare-time project. Phases 1 to 4 make a useful first version and will likely take several months. Dates are not promised.

## Not yet planned, but kept possible

- Desktop PCs (no battery: noise, heat and electricity cost)
- Gaming handhelds and read-only systems such as SteamOS and Fedora Silverblue
- Laptops with two graphics cards
