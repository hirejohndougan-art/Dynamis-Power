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
- [x] Fallback levers per mode, used only when EPP and the platform profile are missing, each through its own probed setting plugin: Longer battery → turbo boost off and a lower speed limit (or a lower maximum frequency where there is no speed limit); Balanced → turbo on, full speed; Faster → turbo on, full speed and the most eager governor the driver offers
- [x] Make modes and learning agree: learning tunes turbo and the speed limit (phase 5), so on such laptops the mode sets the starting point and learning tunes on top for battery, with tuning results kept per mode (as they already are per speed preference); restoring and forgetting put back the mode's value, not a stale one
- [x] Advanced → Profiles: show the fallback levers for each mode so they can be chosen there too
- [x] Say why when a mode cannot be offered ("this laptop has no controls for it"), instead of hiding it silently
- [x] Engine tests on a copy of the synthetic laptop without a speed preference ([decision 0016](docs/decisions/0016-mode-levers-without-epp.md))
- [ ] Try it on the VivoBook

## To do: everyday problems

The plan for each, and why, is in [docs/EVERYDAY-ISSUES.md](docs/EVERYDAY-ISSUES.md) (section numbers below).

- [ ] "Something stopped working": undo the last hour's changes, ask which device misbehaved, block the matching settings (§1)
- [ ] Watch the kernel log on every reading; undo and block a change only when trouble follows it within ten minutes (§1)
- [x] Learning says exactly why it is waiting, with the main reason over the last day and what to do (§2)
- [x] Measure how often the battery updates its power reading; make test blocks at least four update periods long (§2)
- [x] Compute power from the change in energy where a battery reports no power; say so where it reports neither (§2)
- [x] "It feels slow": show what is in effect, undo speed-related changes for the day, optionally make learning stricter and auto mode prefer Balanced (§3)
- [x] Panel tooltip shows auto mode's last reason; Activity groups auto switches per hour (§3)
- [x] A learned change undone by hand twice is blocked on this laptop (§3)
- [x] Notice settings changed behind Dynamis Power's back, name the likely tool, and stop managing a contested setting after two outside changes in a day (§4)
- [x] Detect GNOME without AppIndicator and explain how to show the panel icon (§5)
- [ ] Record kernel and firmware versions with each result; re-check learned settings first after an update; drop settings whose control vanished (§6)
- [ ] Quirk list entries with kernel and firmware version ranges (§6)
- [x] Keep settings chosen by hand (charge limit and start, Advanced "Keep after restart"): re-apply at startup and after sleep, never in safe mode or right after an unclean shutdown (§7)
- [x] Notice when firmware ignores the charge limit, and say so (§7)
- [x] After an unclean shutdown, say what was happening (sleep, wake, driver error, a recent change); if freezes continue in safe mode, say Dynamis Power is unlikely to be the cause (§8)
- [ ] Fedora RPM and Arch PKGBUILD, built by CI (§9)
- [x] Clear message when systemd or polkit is missing (§9)
- [x] "Report a problem": one file with everything useful, privacy filter on, and a pre-filled issue page (§10)

**Follow-up (needs research, real hardware or a decision first):**

- [x] Learning on busy, slow laptops: decided, steady power plus a processor limit scaled by cores ([0020](docs/decisions/0020-quiet-by-steady-power.md))
- [ ] Battery power update rates and noise across makers
- [x] Running from the battery while plugged in: decided, opt-in, standard control only, recalibration first ([0019](docs/decisions/0019-running-from-battery-while-plugged-in.md))
- [x] Importing a TLP configuration: decided, battery values to Longer battery, mains values to Faster, shared values as settings, all shown first ([0017](docs/decisions/0017-importing-a-tlp-configuration.md))
- [x] Each vendor tool: decided, thermald, asusd and supergfxctl run alongside; the rest are offered to switch off ([0022](docs/decisions/0022-other-power-tools.md))
- [x] GNOME: decided, the extension ships in the package; extensions.gnome.org later ([0024](docs/decisions/0024-gnome-extension-in-the-package.md))
- [ ] Which laptops reset or ignore the charge limit
- [x] A freeze long after the last change: decided, it still counts towards safe mode ([0021](docs/decisions/0021-unclean-shutdowns-always-count.md))
- [ ] Does the stall measure catch what people feel as "slow"?
- [x] Desktops, two-GPU laptops, image-based systems, Flatpak: decided ([0023](docs/decisions/0023-desktops-two-gpus-image-based-flatpak.md))
- [x] Wi-Fi power saving without network access: decided, netlink to the local kernel only ([0018](docs/decisions/0018-wifi-power-saving-through-the-kernel-only.md))

**To build, from those decisions:**

- [x] Learning: quiet means steady power for a minute, with the processor limit scaled by cores (0020)
- [x] TLP import: read `tlp.conf` and `tlp.d`, preview line by line, apply after a yes (0017)
- [x] `wifi.power_save` through nl80211; unit allows `AF_NETLINK` without `PrivateNetwork`, still no IP sockets; quirk for RTL8821CE and similar (0018)
- [x] Battery recalibration with `force-discharge`: opt-in, stops at 15%, warmth, unplugging or safe mode, always back to `auto` (0019)
- [x] Other tools: add TUXEDO (tccd) and Slimbook Battery to the switch-off list; leave platform profile and charge limit to asusd while it runs, and the second graphics chip to supergfxctl (0022)
- [x] Desktops: hide battery features without a battery; the welcome says which features apply (0023)
- [x] GNOME extension in the package, with help switching it on (0024)
- [x] Hibernation readiness check and plain advice, in the sleep test report (0025)

## To do: fully replace TLP

Goal: anything TLP manages, Dynamis Power manages too, so TLP can be removed with no loss. Each item becomes a setting plugin (probe, read, apply, revert, risk tier; see docs/SETTINGS.md) that profiles can set and learning can test, or a service behaviour. Items marked *(in SETTINGS)* already have a reserved ID there.

**Devices and buses (the biggest savings TLP has that Dynamis Power lacks)**

- [x] USB autosuspend for idle devices, never for input devices, audio, phones, printers or mobile broadband *(in SETTINGS: `usb.autosuspend`)*
- [x] A per-device "keep awake" list the person can add to (by device model, so it holds in any port)
- [x] Runtime power management for idle PCI devices, with a per-driver "never" list *(in SETTINGS: `pci.runtime_pm`)*
- [x] Wi-Fi power saving on battery *(in SETTINGS: `wifi.power_save`)*; must respect the built-in quirk for the RTL8821CE and similar chips
- [x] Wake-on-LAN off
- [x] Sound controller power saving as well as the codec (`audio.controller_power`)
- [x] Discrete and integrated GPU: a second graphics chip sleeps when idle (`gpu.dgpu_runtime_pm`, learning tests it, never the chip driving the screen), Intel graphics top and lowest speed (`gpu.max_freq`, `gpu.min_freq`, 50 MHz steps; how the driver rounds still to confirm on hardware), AMD graphics power level (`gpu.amd_power_level`)

**Storage**

- [x] SATA link power (`storage.sata_alpm`; "minimum power" not offered)
- [x] AHCI controller runtime power management (part of "Let idle built-in hardware sleep")
- [x] Hard disk power level (APM) for laptops with spinning disks (`storage.disk_apm`, experimental, 128 to 254 or off). Spin-down time not offered: it cannot be read back, so Restore could not put it back
- [x] I/O scheduler (`storage.io_scheduler`)

**Processor**

- [x] Minimum performance level (`cpu.min_perf_pct`)
- [x] Maximum frequency for drivers without performance levels (`cpu.max_freq`, where the driver lists its frequencies)
- [x] Minimum frequency for those drivers (`cpu.min_freq`)
- [x] HWP dynamic boost (`cpu.hwp_dynamic_boost`)
- [x] Scaling governor where the driver offers a choice (`cpu.governor`; modes use it only without a speed preference)

**Radios**

- [x] Switch Bluetooth, Wi-Fi or mobile broadband off on battery, at startup, or Wi-Fi when a network cable is plugged in, as the person chooses (Devices → Radios; rfkill software switches through the engine; rules act only when the situation changes, so a radio switched back on by hand stays on). Remembering on/off across restarts is left to systemd-rfkill, which already does it
- [x] Optional rules on docking and undocking: radios off while docked (an ACPI dock station reporting `docked`, or a Thunderbolt device that calls itself a dock), back on when undocked

**Battery**

- [x] Charge start threshold (`battery.charge_start`)
- [x] Vendor charge controls where the standard file is missing, each found by its own file, never guessed: Lenovo IdeaPad conservation mode (60%), LG (80%), Samsung battery life extender (80%), Sony (50/80%), Huawei (start and stop). ThinkPad, ASUS, Dell, System76, Framework, MSI and Toshiba use the standard file in current kernels
- [x] "Charge fully once" (TLP's `fullcharge`): Battery screen and panel menu
- [x] Battery recalibration on request (TLP's `recalibrate`: needs running from the battery while plugged in; see the follow-up on `charge_behaviour`)

**System**

- [x] Sleep type (`system.mem_sleep`, risky: needs an explicit yes; offered only where both exist)
- [x] Guided sleep test, run only when the person asks (it needs the laptop for about an hour): sleep about 30 minutes with each sleep type the laptop offers (`s2idle`, `deep`), measure the battery used per hour, check that Wi-Fi, sound, the touchpad and other devices came back and that the kernel log shows no resume errors, then recommend the type that used less and woke cleanly (applied only after a yes). Wakes by RTC alarm; plugged-in state, lid and a running call stop it; a failed wake is recorded so that type is not recommended. Results kept per laptop, and later shared with the model's results
- [x] After an unclean shutdown, say whether the laptop was going to sleep or waking when it stopped (from the previous boot's kernel log), to tell sleep problems from other freezes
- [x] Re-apply settings after resume from sleep (mode, kept, learned settings, app shares)
- [x] Apply settings to devices plugged in later (USB, Thunderbolt), as TLP does (at the next reading)
- [x] Ignore a charger that reports wrongly (TLP's `TLP_PS_IGNORE`): Advanced → System
- [x] A mode to use at boot before auto mode decides (TLP's `TLP_DEFAULT_MODE`; today the last chosen mode is used)
- [x] A full diagnostic report like `tlp-stat`: `dynamis stat`, `Diagnostics()`, and in the app's Export
- [x] Import a TLP configuration: read `/etc/tlp.conf` and `/etc/tlp.d/` and offer the matching Dynamis Power settings (the reverse of the advisor's TLP export), as decided in [0017](docs/decisions/0017-importing-a-tlp-configuration.md)

**Done when:** on the test laptops, with TLP removed, `tlp-stat` run once with TLP reinstalled shows nothing TLP would set that Dynamis Power does not, and battery life is at least as long as with a hand-tuned TLP.

## To do: Learn now (a baseline session)

Start learning by hand and leave the laptop alone: every test runs back to back (no waiting for quiet moments, no rests between tests) to set a baseline in about two hours, and learning carries on in everyday use afterwards. Built in the service and the app (Learning screen); the demo plays it out. Decisions taken, as recommended:

- [x] **Idle plus a light, steady workload.** Settings whose cost is lag (turbo, speed limit, speed preference) are measured with a built-in, repeatable light load so lag can be judged; the others idle. The person can untick the workload; those settings are then left for everyday learning.
- [x] **Wait for the screen to turn off** before the first test, so every test is measured in the same state.
- [x] **Battery only** (from at least 40%, 60% recommended). Running from the battery while plugged in stays on the follow-up list.
- [x] **Stops and hands the laptop back** on any keyboard or mouse use (logind idle hint / session agent), plugging in, battery below 40%, warmth, a call or safe mode; finished tests are kept and the rest run later.
- [x] **Keeps the laptop awake** during the session (a logind `idle:sleep` block inhibitor, released at the end); the lid still sleeps it.
- [x] Service: `LearnSession()`, `StartLearnSession(workload)`, `StopLearnSession()`; the session reuses calibration (`calibrate.rs`) with cooldown 0 and blocks sized to the battery's update period.
- [ ] **Per-activity learning afterwards:** keep results separately for idle, light use, video and heavy work, and apply each in its own context (needs a decision record).

## To do: from the Power Daemon design (the earlier "Building a project with GUI" chat)

That design and Dynamis Power share the goal; this is what it had that Dynamis Power did not, and what happens to each idea.

**Already covered here:** systemd service with D-Bus and one package for service and app; three modes plus auto mode by workload; per-app power accounting and per-app modes; app power hints (power-profiles holds); low-battery override; learning that tests on this laptop and keeps only what measurably helps, with rollback (journal, trials, undo, safe mode); learning progress shown to the person; privacy-first local data; native packages (`.deb` now, RPM and AUR on the list); shared results (phase 8, through the separate signed helper).

**To add:**

- [ ] **Temperature on Home** next to power use (the hottest sensor; built: `power.temp_c` in `Status`), and a temperature line on the chart.
- [ ] **Heat that is building up:** when the temperature trend says the laptop will be hot within a few minutes on battery, auto mode moves to a cooler mode early instead of waiting for the firmware to throttle; Activity says why. Only modes and speed limits, never thermal trip points or fans.
- [ ] **"Runs hotter than it used to":** compare temperature at the same load over months; a steady rise suggests dust or old thermal paste, and the app says so in plain words (Battery or Home).
- [ ] **Learn from the person's choices:** when the person picks the same mode by hand in the same situation (an app, a time, plugged in or not) a few times, offer to make it an automatic rule ("You chose Faster three times while using Blender. Do that automatically?"). Never applied without a yes.
- [ ] **Time-of-day habits in auto mode:** use the habits already learned for charging (hour of the week) to prefer a mode at usual times, as a suggestion first.
- [ ] **Re-check learned results over time** (every few months, and after a kernel or firmware update, see everyday problem 6): seasons, dust and updates change what is best.
- [ ] **Graphics speed per activity:** test the graphics power level (`gpu.amd_power_level` low/auto now; Intel minimum and maximum graphics frequency, on the TLP list) in learning, within the driver's own limits, per activity.
- [ ] **Brightness through the desktop:** setting brightness through the kernel works everywhere, but KDE Plasma's brightness slider may show the old level until it is next moved. Have the session agent set it through the desktop (`org.kde.ScreenBrightness` on Plasma 6, GNOME's settings daemon) when it is running, and fall back to the service.
- [ ] **App power trend:** "Firefox uses 20% more power than last month" from the per-app energy totals, kept locally.
- [ ] **What wakes the laptop:** list wake-up sources and wake reasons after sleep (`/sys/power/wakeup_sources`, the kernel log), to explain overnight drain.
- [x] **Sleep that turns into hibernation when the battery runs low:** advice only, never set up by the service ([0025](docs/decisions/0025-hibernate-on-low-battery-is-advice.md)).
- [ ] **Signals for the app:** `ThermalAlert`, `AppDetected`, `LearningProgress` on D-Bus, so the app and panel icon update without asking.
- [ ] **Shared hardware profiles** (phase 8): a new laptop starts from what similar laptops learned; still opt-in, through the separate signed helper, publishing only per-model results (as in `schema/`), never per-person data. Federated averaging of model weights is not needed: the results are already small, per-setting measurements.

**Left out, by the safety rules** ([docs/SAFETY.md](docs/SAFETY.md)): changing graphics or processor voltage, clocks beyond the driver's own range, fan speed, thermal trip points and switching processor cores off (rule 5); the service talking to the network (rule 6).

**Not taken, by design:** a trained prediction model (XGBoost) choosing settings by itself. Dynamis Power decides from measured, repeatable tests that can each be explained ("saves 0.4 W, no lag") and undone; a model's choices cannot. Calendar access for predictive hibernation (personal data the service does not need).

## Realistic expectations

This is a spare-time project. Phases 1 to 4 make a useful first version and will likely take several months. Dates are not promised.

## Not yet planned, but kept possible

- Desktop PCs (no battery: noise, heat and electricity cost)
- Gaming handhelds and read-only systems such as SteamOS and Fedora Silverblue
- Laptops with two graphics cards
