# Roadmap

Dynamis Power is built in eight phases. Each phase is usable on its own and ends with a gate that must pass before the next one starts.

| Phase | Builds | Gate: done when |
| --- | --- | --- |
| 1. Measure **(in progress)** | Logger, hardware inventory, power report | Readings are trustworthy and the daemon stays within its overhead budget |
| 2. Profiles **(built; gate waits on hardware testing)** | Setting plugins, standard D-Bus profiles, change journal, quirk list, clean uninstall | The desktop power slider works and every change reverts |
| 3. Auto-switch | Session agent, context rules, workload detection | The right profile is chosen in scripted scenarios |
| 4. Calibrate | Per-setting tests, display power model | Beats stock "Balanced" on battery |
| 5. Learn | Optimiser for interacting CPU settings | Beats a hand-tuned TLP configuration |
| 6. Apps | Per-app accounting, foreground boost, background throttle | The focused app stalls less under load |
| 7. Predict | "Last until" mode, adaptive charging, habits | Time targets are met within a margin |
| 8. Release | Full app, shared results, optional AI helper | Public release with published benchmark data |

Phase 2 is built: setting plugins, journal, trials with a health monitor, restore, the three standard profiles, the `dynamisd` service with the standard D-Bus interface, the signed quirk list and kill switch, safe mode after repeated unclean shutdowns, and install and clean uninstall. It was built while the phase 1 hardware test is pending, because it can be tested entirely on fixtures. Phase 2 does not count as done, and nothing from it should run on real hardware by default, until the phase 1 gate passes.

**Phase 4 is the go/no-go point.** If per-setting calibration cannot beat stock settings on battery, the approach gets rethought before more is built.

## Realistic expectations

This is a spare-time project. Phases 1 to 4 make a useful first version and will likely take several months. Dates are not promised.

## Not yet planned, but kept possible

- Desktop PCs (no battery: noise, heat and electricity cost)
- Gaming handhelds and read-only systems such as SteamOS and Fedora Silverblue
- Laptops with two graphics cards
