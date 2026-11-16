# Architecture

Dynamis Power is a small privileged daemon, a per-user session agent, and thin front ends. Only the daemon is required; everything else talks to it over D-Bus.

```
 Desktop power slider      Dynamis app (Tauri + React)      Session agent (per user)
 and apps holding          CLI and tray                     focused app, media,
 performance                                                camera, idle state
          \                         |                          /
           \________________________|_________________________/
                                    |
                    D-Bus (system bus)
     org.freedesktop.UPower.PowerProfiles   (standard, desktop-compatible)
     net.hadess.PowerProfiles               (older name, still used by some tools)
     org.dynamispower.Daemon1               (extended Dynamis interface — planned name)
                                    |
 +------------------------------ dynamisd (root) ------------------------------+
 |  Fast loop          Learner              Journal + rollback                   |
 |  context rules,     calibration,         every change logged                  |
 |  workload detect    optimisation         before it is applied                 |
 |                                                                              |
 |  Sensor plugins     Setting plugins      Data store (SQLite, size-capped)     |
 +------------------------------------------------------------------------------+
                                    |
              Kernel: sysfs, procfs, cgroups v2, powercap, udev events

 Separate, unprivileged:  dynamis-sync — downloads the signed quirk list and
                          shared summaries; uploads results only when the user opts in.
```

## Current state

Code lives in `daemon/crates/`: `dynamis-core` (sensors, inventory, log format, setting plugins, the change engine and the standard profiles), `dynamis-cli` (the `dynamis` command) and `dynamisd` (the system service serving the standard power-profiles D-Bus interface). Install files are in `daemon/dist/`. Phase 1 (measure) is waiting on its real-hardware test; phase 2 has setting plugins, the journal, trials, crash recovery, restore, safe mode, the three standard profiles and the service. There is no fast loop, learner or app yet. Everything below marked *planned* does not exist yet.

How a change flows today:

```
dynamis set cpu.epp balance_power --trial 60
  └─ Engine::apply  (engine.rs)
       safe mode? blocked? risk opt-in? probe allows the value?
       baseline.json  ← original value, first time only
       pending.json + journal.jsonl "start"   (flushed to disk)
       writer::write_all  (writer.rs, the only write path)
       read back and compare; on mismatch put the old value back
  └─ "Keep this change?"  y → journal "keep"   anything else → journal "revert"
```

## Components

| Component | Runs as | Job | Code |
| --- | --- | --- | --- |
| `dynamisd` | root, systemd service | Reads sensors, applies settings, runs both loops, keeps the journal | `daemon/` |
| Session agent | each logged-in user | Reports focused app, media playback, camera and microphone use, idle state | `daemon/` (separate binary, planned) |
| `dynamis-sync` | unprivileged system user | All network traffic: quirk list, shared summaries, opt-in uploads | `daemon/` (separate binary, planned) |
| `dynamisctl` | user | Command line: status, reports, profile switch, revert | `daemon/` (planned) |
| App | user, on demand | Dashboard and controls | `gui/` |
| Results schema | — | Format of shared results | `schema/` |
| Submission service | Cloudflare Worker | Receives opt-in results, validates, stores, publishes summaries | separate repository (planned) |

**Why the network is split out:** the root daemon never opens a network connection. `dynamis-sync` runs without privileges, verifies signatures, and hands verified files to the daemon. A bug in network code therefore cannot become a root exploit.

## Plugins

Plugins are the unit of hardware support. Adding support for a new device or setting means adding a plugin, not changing the core.

**Sensor plugin** — reads one source.

```
probe()  -> Available | Unavailable(reason)
read()   -> Sample { value, unit, timestamp }
```

**Setting plugin** — owns one lever.

```
id          e.g. "cpu.epp"           (stable; used in journals and shared results)
risk_tier   Safe | UsuallySafe | Risky | Experimental | Advisory
probe()     -> Supported { allowed_values or range } | Unsupported(reason)
read()      -> current value
apply(v)    -> Ok | Err      (only values from probe(); journaled first)
revert()    -> restores the value recorded at daemon start
```

The setting `id` is permanent. Renaming one breaks journals, learned data and shared results, so treat it like a public API. See [SETTINGS.md](SETTINGS.md) for the catalogue.

## The two loops

- **Fast loop (seconds):** reacts to context — charger, battery level, video playing, a call starting — and picks the profile. Rule-based and predictable.
- **Slow loop (days):** the learner. Tries one small change at a time when conditions allow, measures it, keeps or reverts it. See [DESIGN.md](DESIGN.md).

## Data on disk

| Path (planned) | Contents | Format |
| --- | --- | --- |
| `/var/lib/dynamis-power/journal.jsonl` **(built)** | Every change: `start`, `done`, `failed`, `keep`, `revert`, `recover`, `restore`, `block`, `unblock` | JSON Lines, `"v": 1`, fsynced ([0007](decisions/0007-plain-file-change-journal.md)) |
| `/var/lib/dynamis-power/pending.json` **(built)** | The change in flight or under trial | JSON, `"v": 1`; present at start-up means recover |
| `/var/lib/dynamis-power/baseline.json` **(built)** | Original value of each changed setting | JSON, `"v": 1`; never overwritten |
| `/var/lib/dynamis-power/blocked.json` **(built)** | Settings blocked after an unfinished change | JSON, `"v": 1` |
| `/var/lib/dynamis-power/lock` **(built)** | Kernel file lock while changing settings; holder's PID as a hint | Text |
| `/var/lib/dynamis-power/profile.json` **(built)** | The profile the user last chose | JSON, `"v": 1` |
| `/var/lib/dynamis-power/replaced-services` **(built)** | Services `install.sh` turned off, for uninstall | Text, one per line |
| `/var/lib/dynamis-power/state.db` | Samples, experiment records, learned settings | SQLite, versioned with migrations |
| `/var/lib/dynamis-power/quirks/` | Downloaded signed quirk list | Signed JSON |
| `/etc/dynamis-power/config.toml` | Admin settings and pinned values | TOML |
| `/etc/dynamis-power/disable` **(built)** | If present, every change is refused (safe mode) | Empty file |

## Footprint budget

The daemon must cost less than it saves: under 20 MB of memory, under 0.5% average CPU, and no periodic wakeups when nothing is changing. The app reports the daemon's own cost.

## Coexistence

Dynamis Power replaces power-profiles-daemon, TLP, auto-cpufreq and TuneD, which change the same kernel settings. The installer detects them and offers to disable them. An advisor mode changes nothing and writes its findings as a TLP or TuneD config instead.
