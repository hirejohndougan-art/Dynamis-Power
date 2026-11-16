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
| 7. Predict | "Last until" mode, adaptive charging, habits | Time targets are met within a margin |
| 8. Release | Full app, shared results, optional AI helper | Public release with published benchmark data |

Phase 2 is built: setting plugins, journal, trials with a health monitor, restore, the three standard profiles, the `dynamisd` service with the standard D-Bus interface, the signed quirk list and kill switch, safe mode after repeated unclean shutdowns, and install and clean uninstall. It was built while the phase 1 hardware test is pending, because it can be tested entirely on fixtures. Phase 2 does not count as done, and nothing from it should run on real hardware by default, until the phase 1 gate passes.

Phase 3 is built: auto mode with rules over eight workload classes, the session agent, `dynamis auto`, and the scripted scenarios in `tests/scenarios/`, which form its gate and pass. It still needs trying on a real desktop (docs/TESTING-ON-HARDWARE.md, phase 3 section).

Phase 4 is built: calibration experiments, learned settings applied on battery, the display power model, and `dynamis learn` / `dynamis learned`. On a simulated laptop it finds real savings and rejects changes that add lag. Whether it beats stock Balanced on real laptops is what the gate measures.

Phase 5 is built: once every setting has been tested on its own, turbo boost and the speed limit are tuned together, one step at a time, per power mode, with a lag limit for the whole set ([decision 0014](docs/decisions/0014-tuning-together-one-step-at-a-time.md)). On a simulated laptop where turbo off only helps below an 80% speed limit, it finds that combination; testing each setting once does not. Whether it beats a hand-tuned TLP setup is what the gate measures, on real laptops.

Phase 6 is built: per-app energy estimates, priority for the app in front, slowing background apps on battery, per-app "always slow" and "never slow", per-app modes in auto mode, and the Apps screen. It only ever changes an app's share of the processor, never caps or freezes it ([decision 0013](docs/decisions/0013-app-shares-not-caps.md)). Its gate (the app in front stalls less under load) needs a real laptop.

The desktop app and `.deb` packages for it and the service (part of phase 8) were started early, because it does not depend on the phase 4 result and makes phases 1–4 usable without a terminal: Home, Battery, Activity with export, and Advanced. See [gui/README.md](gui/README.md).

**Phase 4 is the go/no-go point.** If per-setting calibration cannot beat stock settings on battery, the approach gets rethought before more is built.

## Realistic expectations

This is a spare-time project. Phases 1 to 4 make a useful first version and will likely take several months. Dates are not promised.

## Not yet planned, but kept possible

- Desktop PCs (no battery: noise, heat and electricity cost)
- Gaming handhelds and read-only systems such as SteamOS and Fedora Silverblue
- Laptops with two graphics cards
