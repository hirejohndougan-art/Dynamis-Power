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
