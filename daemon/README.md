# daemon

The Rust side of Dynamis Power. Today it holds phase 1: read-only sensors, a hardware inventory and the `dynamis` command line. The system service (`dynamisd`) comes in phase 2.

**Nothing here changes any system setting.** Every command only reads.

## Layout

```
daemon/
  Cargo.toml                 workspace
  crates/
    dynamis-core/            library: sensors, inventory, log format
      src/fs.rs              read-only access to /sys and /proc under a configurable root
      src/sensors/           one module per signal: power, cpu, pressure, thermal, rapl, backlight
      src/inventory.rs       what the machine is and which settings it exposes
      src/logfmt.rs          CSV / JSON Lines rows built from two snapshots
      tests/                 tests against tests/fixtures/sysfs/
    dynamis-cli/             the `dynamis` command
```

Everything reads through `SysRoot`, so tests point it at a fixture directory instead of the real system. See [../tests/fixtures/README.md](../tests/fixtures/README.md).

## Building

You need Rust 1.80 or newer. On Ubuntu, KDE neon or Debian:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # installs Rust; follow the prompts
source "$HOME/.cargo/env"
git clone https://github.com/hirejohndougan-art/dynamis-power
cd dynamis-power/daemon
cargo build --release
```

The program is then at `target/release/dynamis` (under 1 MB).

## Using it

```sh
./target/release/dynamis inventory          # what this machine is and what can be tuned
./target/release/dynamis status             # power, speed, lag and temperature right now
./target/release/dynamis log -o power.csv   # record a reading every 5 s until Ctrl+C
```

Useful options:

| Command | Option | Meaning |
| --- | --- | --- |
| `inventory`, `status` | `--json` | Machine-readable output |
| `status` | `--over 5` | Measure over 5 seconds instead of 2 |
| `log` | `--interval 10` | Seconds between readings |
| `log` | `--duration 3600` | Stop after an hour |
| `log` | `--format jsonl` | JSON Lines instead of CSV |

**Run with `sudo` to see CPU power.** On recent kernels the CPU energy counters can only be read by root. Everything else works as a normal user. Reading is all the program does, with or without `sudo`.

### Log columns

The CSV starts with `# dynamis log format 1`, then these columns. Each row covers the time since the previous row.

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

### Adding a sensor

1. Add a module under `crates/dynamis-core/src/sensors/` that reads through `SysRoot` and never writes.
2. Return `Option` or empty lists for anything a machine may not have. A missing file is normal, not an error.
3. Add it to `Snapshot` in `sensors/mod.rs`.
4. Add the files it reads to a fixture, and a test in `crates/dynamis-core/tests/`.
5. If it adds a log column, append it to `Row`, its CSV header and `to_csv`, and bump `LOG_FORMAT_VERSION`.
6. Document it in [../docs/MEASUREMENT.md](../docs/MEASUREMENT.md).

### Adding a lever to the inventory

Add a probe in `levers()` in `inventory.rs` using the permanent setting ID from [../docs/SETTINGS.md](../docs/SETTINGS.md), then a test.
