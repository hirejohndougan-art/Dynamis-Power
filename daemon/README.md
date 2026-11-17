# daemon

The Rust side of Dynamis Power. Today it holds phase 1 (sensors, a hardware inventory, logging) and phase 2 so far: setting plugins, a safe change engine, the three standard power profiles, and the `dynamisd` system service that the desktop's power slider talks to.

**Only `set`, `restore` and `unblock` change anything.** Every other command only reads.

## Layout

```
daemon/
  Cargo.toml                 workspace
  crates/
    dynamis-core/            library: sensors, inventory, log format
      src/fs.rs              read-only access to /sys and /proc under a configurable root
      src/sensors/           one module per signal: power, cpu, pressure, thermal, rapl, backlight
      src/inventory.rs       what the machine is and which settings it exposes
      src/settings/          setting plugins: probe, read, and plan a change (never write)
      src/engine.rs          applies changes: checks, journal, verify, trials, recovery, restore
      src/writer.rs          the only code that writes to the kernel (private to the crate)
      src/logfmt.rs          CSV / JSON Lines rows built from two snapshots
      src/report.rs          summaries of a recorded log
      src/gauge.rs           battery gauge update rate and resolution
      src/selfstat.rs        the process's own CPU and memory use
      tests/                 tests against tests/fixtures/sysfs/
      src/profiles.rs        power-saver / balanced / performance, chosen from what the machine offers
      src/health.rs          watches the kernel log, stalls and temperature during a trial
      src/quirks.rs          built-in and signed downloaded quirk lists
      src/boots.rs           counts unclean shutdowns for automatic safe mode
      src/auto.rs            auto mode's rules (pure; replayed by tests/scenarios/)
      src/context.rs         auto mode's system readings: charger, battery, CPU, network, microphone, power, lag
      src/calibrate.rs       calibration experiments and their statistics (pure; simulated in tests)
      src/display.rs         display power model
      quirks/builtin.json    the built-in quirk list
    dynamis-cli/             the `dynamis` command
    dynamis-sync/            downloads the signed quirk list (unprivileged; the only network use)
    dynamis-quirktool/       maintainers only: signing keys and signing the quirk list
    dynamis-agent/           per-user session agent: media, screen lock, games (for auto mode)
    dynamisd/                the system service: standard power-profiles D-Bus interface, auto mode
      src/core.rs            chosen profile, holds, applying (no D-Bus; unit-tested)
      src/bus.rs             the D-Bus objects and polkit checks
      tests/dbus.rs          end-to-end test on a private bus and a fixture copy
  dist/                      systemd unit, D-Bus policy, polkit actions, install.sh, uninstall.sh
```

Everything reads through `SysRoot`, so tests point it at a fixture directory instead of the real system. See [../tests/fixtures/README.md](../tests/fixtures/README.md).

## Building

You need Rust 1.89 or newer. On Ubuntu, KDE neon or Debian:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # installs Rust; follow the prompts
source "$HOME/.cargo/env"
git clone https://github.com/hirejohndougan-art/dynamis-power
cd dynamis-power/daemon
cargo build --release
```

The programs are then at `target/release/dynamis` (under 1 MB) and `target/release/dynamisd` (about 2.5 MB).

## Using it

```sh
./target/release/dynamis inventory          # what this machine is and what can be tuned
./target/release/dynamis status             # power, speed, lag and temperature right now
./target/release/dynamis log -o power.csv   # record a reading every 5 s until Ctrl+C
./target/release/dynamis report power.csv   # summarise a recording
./target/release/dynamis gauge              # how often the battery gauge updates (run on battery)
./target/release/dynamis hardware-report    # text to paste into a GitHub hardware report
./target/release/dynamis stat               # full diagnostic report, like tlp-stat (sudo for everything)
./target/release/dynamis sleep-test on      # guided sleep test (the laptop sleeps and wakes by itself); `dynamis sleep-test` for results
./target/release/dynamis settings           # what can be changed on this machine, and current values
```

### Changing settings

```sh
sudo ./target/release/dynamis set vm.swappiness 20 --trial 60   # undone after 60 s unless you type y
sudo ./target/release/dynamis set cpu.turbo off                  # applied directly
sudo ./target/release/dynamis journal                            # every change, with old and new values
sudo ./target/release/dynamis restore                            # put everything back how it was
sudo ./target/release/dynamis quirks                             # settings not allowed on this laptop, and why
sudo ./target/release/dynamis safe-mode on|off                   # stop or allow all changes
```

During a `--trial`, Dynamis Power watches the kernel log, memory and disk stalls, and temperature every second. Any trouble undoes the change at once and blocks that setting.

- Changes last until reboot. Nothing is written to permanent configuration.
- **Use `--trial` while trying things.** If the laptop freezes before you confirm, the change is undone and that setting is blocked the next time `dynamis` changes anything (`dynamis unblock <id>` allows it again). A reboot also undoes it.
- Values outside what the machine and Dynamis Power allow are refused before anything is touched.
- Safe mode: `sudo dynamis safe-mode on` refuses every change (`restore` still works). It also switches on by itself after three unclean shutdowns in a row, and stays on until `sudo dynamis safe-mode off`. The `/etc/dynamis-power/disable` file and the `dynamis.safe=1` boot option do the same from outside.
- State is kept in `/var/lib/dynamis-power/`; see [../docs/decisions/0007-plain-file-change-journal.md](../docs/decisions/0007-plain-file-change-journal.md).

On a laptop with unexplained freezes (like the project's VivoBook), a freeze during a trial does not prove the setting caused it. Check `dynamis journal` and the system log before drawing conclusions.

For the full phase 1 test on a real laptop, run `sudo ./scripts/phase1-test.sh`, or follow [../docs/TESTING-ON-HARDWARE.md](../docs/TESTING-ON-HARDWARE.md).

Useful options:

| Command | Option | Meaning |
| --- | --- | --- |
| `inventory`, `status` | `--json` | Machine-readable output |
| `status` | `--over 5` | Measure over 5 seconds instead of 2 |
| `log` | `--interval 10` | Seconds between readings |
| `log` | `--duration 3600` | Stop after an hour |
| `log` | `--format jsonl` | JSON Lines instead of CSV |
| `log` | `--sync` | Force each reading to disk, so the log survives a hard freeze |
| `report` | `--json` | Machine-readable summary |
| `gauge` | `--duration 300` | Watch the gauge for 5 minutes |
| `set` | `--trial 60` | Undo after 60 s unless confirmed |
| `set` | `--allow-risky` | Needed for Risky and Experimental settings |
| `set` | `--reason "..."` | Note for the journal |
| `journal` | `--last 100`, `--json` | More entries; JSON Lines |

**Run with `sudo` to see CPU power.** On recent kernels the CPU energy counters can only be read by root. Everything else works as a normal user. Reading is all the program does, with or without `sudo`.

## Installing from packages

The easy way, with no compiling on the laptop once the packages exist:

```sh
cd daemon && ./packaging/build-deb.sh                  # the service package
cd ../gui && npm install && npx tauri build --bundles deb   # the app package
cd .. && sudo apt install ./daemon/target/deb/dynamis-power-service_*.deb ./gui/src-tauri/target/release/bundle/deb/*.deb
```

Or download both `.deb` files from a CI run's *Artifacts* and install them the same way. `sudo apt remove dynamis-power dynamis-power-service` restores every original setting and turns power-profiles-daemon back on. Details: [packaging/README.md](packaging/README.md).

## Running the service

`dynamisd` replaces power-profiles-daemon: it serves the same D-Bus interface, so KDE's battery applet, GNOME's power menu and `powerprofilesctl` work unchanged.

```sh
cargo build --release
sudo ./dist/install.sh        # shows what it will do and asks first
powerprofilesctl              # should list the profiles, driver "dynamis"
journalctl -u dynamisd        # what it did and why
sudo ./dist/uninstall.sh      # restores every setting and turns power-profiles-daemon back on
```

- Today a profile changes the firmware power mode and the CPU speed preference, where the machine has them. Nothing is learned yet.
- The profile you choose is remembered across reboots and re-applied at start.
- `sudo dynamis restore` puts the original values back at any time, with or without the service.
- The service refuses to start if power-profiles-daemon or tuned-ppd is running, before touching anything. `install.sh` stops and masks them, and `uninstall.sh` turns them back on.
- If a change was interrupted (a freeze, a power cut), the service undoes it at start and blocks that setting.

### Auto mode

```sh
dynamis auto on        # the service picks the profile from what you are doing
dynamis auto           # what it sees and why: "Workload: video — Playing media: keeping it smooth"
dynamis auto off
```

- Off until you switch it on. Picking a mode in the panel pauses it until the charger is plugged in or out.
- Media, screen lock and games are reported by `dynamis-agent`, which starts at login (log out and in once after installing).
- The rules and every scenario they are tested against are in `crates/dynamis-core/src/auto.rs` and `../tests/scenarios/`. To add a case, copy a scenario file. `sudo dynamis journal` shows it; `sudo dynamis unblock <id>` allows it again.

### The service's own D-Bus interface

`net.dynamis.Power` at `/net/dynamis/Power`, interface `net.dynamis.Power1`. Reads need no permission; changes ask polkit (the person at the laptop is allowed). The app uses it through `crates/dynamis-client`.

| Member | Kind | What |
| --- | --- | --- |
| `AutoMode`, `Learning` | property (read/write) | Switch auto mode or learning |
| `Workload`, `AutoReason`, `LearningStatus` | property | What auto mode sees and why; what learning is doing |
| `Status()` | method → JSON | Everything the app's Home and Battery screens show |
| `Journal(n)` | method → JSON | Last `n` journal entries, newest first, with plain names |
| `Settings()`, `Inventory()`, `LearnedJson()` | method → JSON | Settings overview; hardware summary; test results and display model |
| `Undo(setting)`, `Forget(setting)` | method | Put one setting back; forget what was learned about it |
| `Restore()` | method → JSON | Restore every original setting (switches auto mode and learning off; works in safe mode) |
| `SetSetting(setting, value, allow_risky)`, `SetSafeMode(on)` | method | Change one setting; safe mode on or off |
| `AutoPrefs()`, `SetAutoPrefs(json)` | method | Auto mode's preferences; out-of-range values are refused with a reason |
| `Conflicts()`, `SwitchOffConflict(unit)` | method | Other power tools found running or enabled (asked of systemd); switch one off (administrator password; only known tools; recorded so removal turns it back on) |
| `SleepTest()`, `StartSleepTest(minutes)`, `StopSleepTest()` | method | The guided sleep test: progress and results (JSON); start (5–60 minutes per sleep type; battery only); stop and put the original sleep type back |
| `Diagnostics()` | method → text | The full diagnostic report (`dynamis stat`) |
| `ChargeFullyOnce(on)` | method | Lift the charge limit until the battery is full or the charger is unplugged, or stop |
| `Recalibration()`, `StartRecalibration()`, `StopRecalibration()` | method | Battery recalibration: with the charger plugged in, run down to 15% (`force-discharge`), then charge fully. `Recalibration` says whether it can run here, its phase (`running_down`, `charging`) and the last result (what the gauge said the battery holds before and after) |
| `SomethingBroke()`, `WhatBroke(kind)` | method | Something stopped working: `SomethingBroke` undoes every change from the last hour and holds those settings until an answer (or 12 hours); `WhatBroke` takes `wifi`, `bluetooth`, `sound`, `input`, `screen`, `storage`, `sleep`, `freeze`, `other` or `none`, puts back and blocks only the settings that could cause it and were changed, and returns them. `trouble` in `Status` while waiting | Trouble in the kernel log within 10 minutes of a change that could cause it undoes and blocks that change by itself (`kernel` in the journal; `kernel_watch` in `Status` says whether the log can be read).
| `SetLearnDuringHeavyWork(on)` | method | Also learn speed settings during heavy work (decision 0026; off by default). `learning.heavy`, `learning.activity` and `learning.activity_tests` in `Status` |
| `AnswerSuggestion(id, yes)` | method | Yes or no to `suggestion` in `Status` (a rule offered from modes picked by hand: an app, or a time of day). Yes adds the rule and switches automatic choice on; no is remembered |
| `Sleeps()` | method | The last sleeps (newest first): start and end, battery before and after, battery used per hour, and the wake-up sources that woke it with a plain kind (`lid`, `usb`, `network`, …) |
| `TlpImport()`, `ApplyTlpImport(au)` | method | Importing TLP's configuration (`/etc/tlp.d/*.conf`, then `/etc/tlp.conf`): the preview line by line (setting, value, where it goes: a kept setting, Longer battery or Faster; or why not imported), then apply the lines at the given positions |
| `LearnSession()`, `StartLearnSession(workload)`, `StopLearnSession()` | method | *Learn now*: every test back to back while the laptop is left alone on battery (from 40%); waits for the screen to turn off; `workload` measures turbo and the speed limit under a built-in light load; stops on use, charger, low battery, warmth, a call or safe mode; holds a logind sleep inhibitor meanwhile. `LearnSession` gives the phase (`idle`, `waiting_screen`, `testing`, `done`, `stopped`), progress and results |
| `Brightness()`, `SetBrightnessPrefs(json)` | method | The screen brightness chosen for each mode (`{"levels": {"power-saver": 40}}`; a mode left out is left alone), whether it can be set here and the level now. Set once when a mode starts; a new level for the mode in use at once |
| `Devices()`, `SetDevicePolicy(json)` | method | USB devices and PCI functions that can sleep when idle, with why some are kept awake; the switches (`{"usb": bool, "pci": bool, "never": [...], "deeper_link_sleep": bool}`; deeper link sleep is Experimental and the app asks first). Each PCI device has `link_sleep` (null where its link has no L1 substates) |
| `CheckDeepIdle()` | method | Start a one-minute, read-only check of how deeply the processor rests; `deep_idle` in `Status` has the progress and the last report (deepest core and package idle share, devices kept awake, links that never sleep, links able to sleep more deeply) |
| `SetDesktopPolicy(json)` | method | Acting through the desktop (decision 0027): `{"pause_indexing": bool, "lower_refresh": bool, "dark_style": bool}`. `desktop` in `Status` has the choices, what is wanted now, what the session agent found and whether the screen looks like an OLED |
| `Desktop()`, `ReportDesktop(json)` | method | For the session agent only: what to do now, and what this desktop can do |
| `ReportSession(facts)` | method | For the session agent only |

### Learning

```sh
dynamis learn on       # test settings one at a time at quiet moments on battery
dynamis learn          # what it is doing: "Testing kernel.nmi_watchdog = off: step 3 of 8"
dynamis learned        # every result, what was kept, and the screen's power cost
dynamis learned forget kernel.nmi_watchdog
dynamis learn off      # stops testing and puts learned settings back
```

- Tests run only on battery at 40% or more, during idle or light use, and stop if anything changes. Each takes about 8 minutes.
- A change is kept only if it saves power with confidence and adds no lag. Learned settings apply on battery only.
- How it decides: [../docs/decisions/0012-calibration-method.md](../docs/decisions/0012-calibration-method.md).

### Log columns

The CSV starts with `# dynamis log format 2`, then these columns. Each row covers the time since the previous row.

| Column | Meaning |
| --- | --- |
| `time` | End of the interval, seconds since 1970 |
| `interval_s` | Length of the interval |
| `on_ac` | Charger connected |
| `battery_pct` | Battery charge |
| `battery_w` | Whole-system power from the battery gauge (accurate on battery only) |
| `cpu_pkg_w` | CPU package power from the energy counters (needs root) |
| `cpu_avg_mhz` | Average CPU frequency |
| `turbo` | Turbo boost on or off |
| `epp` | CPU speed preference |
| `stall_cpu_pct`, `stall_mem_pct`, `stall_io_pct` | Share of the interval in which tasks waited for CPU, memory or disk — the lag measure |
| `max_temp_c` | Hottest temperature sensor |
| `brightness_pct` | Screen brightness |
| `self_cpu_pct` | CPU used by the logger itself, as % of one core (format 2) |
| `self_rss_mb` | Memory used by the logger itself (format 2) |

Columns are only ever added at the end, with a format version bump. See `LOG_FORMAT_VERSION` in `crates/dynamis-core/src/logfmt.rs`.

## Developing

```sh
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

Try the command against the test fixture instead of your machine:

```sh
cargo run -- --root ../tests/fixtures/sysfs/synthetic-laptop inventory
```

To try changes, **copy the fixture first** so the checked-in files never change:

```sh
cp -a ../tests/fixtures/sysfs/synthetic-laptop /tmp/fx
cargo run -- --root /tmp/fx --state-dir /tmp/fx-state set vm.swappiness 10 --trial 10
```

### Adding a sensor

1. Add a module under `crates/dynamis-core/src/sensors/` that reads through `SysRoot` and never writes.
2. Return `Option` or empty lists for anything a machine may not have. A missing file is normal, not an error.
3. Add it to `Snapshot` in `sensors/mod.rs`.
4. Add the files it reads to a fixture, and a test in `crates/dynamis-core/tests/`.
5. If it adds a log column, append it to `Row`, its CSV header and `to_csv`, and bump `LOG_FORMAT_VERSION`.
6. Document it in [../docs/MEASUREMENT.md](../docs/MEASUREMENT.md).

Run the service against a fixture copy on a private bus, with no polkit and no root:

```sh
cp -a ../tests/fixtures/sysfs/synthetic-laptop /tmp/fx
dbus-daemon --session --address=unix:path=/tmp/fx-bus --nofork &
DBUS_SESSION_BUS_ADDRESS=unix:path=/tmp/fx-bus cargo run -p dynamisd -- --session --root /tmp/fx --state-dir /tmp/fx-state
# in another terminal, with the same DBUS_SESSION_BUS_ADDRESS:
busctl --user set-property org.freedesktop.UPower.PowerProfiles /org/freedesktop/UPower/PowerProfiles \
  org.freedesktop.UPower.PowerProfiles ActiveProfile s power-saver
```

`--session` only works together with `--root`, so the service can never change the real system without polkit.

### Adding a setting plugin

See "Adding a plugin" in [../docs/SETTINGS.md](../docs/SETTINGS.md#implemented-plugins).

### Adding a lever to the inventory

Add a probe in `levers()` in `inventory.rs` using the permanent setting ID from [../docs/SETTINGS.md](../docs/SETTINGS.md), then a test.
