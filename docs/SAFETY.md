# Safety rails

Dynamis Power changes system settings automatically, so it must never leave a machine worse than it found it. The rails work in layers: if one misses a problem, the next catches it.

## Risk tiers

Every setting plugin declares one tier.

| Tier | Meaning | Default handling |
| --- | --- | --- |
| Safe | Cannot cause data loss or instability | Applied and explored automatically |
| Usually safe | Fine on most hardware, with known exceptions | Applied after a per-device test; reverted on any fault |
| Risky | Can cause flicker, disconnects or data loss on some hardware | Off until the user opts in, setting by setting |
| Experimental | Benefit unproven | Off by default |
| Advisory | Needs a reboot, bootloader edit or app setting | Never applied; explained to the user |

## Rails by stage

| Stage | Rail | What it prevents |
| --- | --- | --- |
| Before a change | Probe first: only settings this machine exposes, within the driver's own minimum and maximum | Writing values the hardware never offered |
| Before a change | Signed quirk blocklist, downloaded separately from app updates | Retrying a setting known to break this model |
| Before a change | Pre-flight checks: battery above a floor, normal temperature, no call, full-screen video or heavy job, user idle | Experiments at a bad moment |
| During a test | One change at a time, small steps, a limit on changes per hour | Untraceable combinations and flapping |
| During a test | Health monitor: kernel errors (GPU hangs, disk timeouts, USB and Wi-Fi disconnects), stall spikes, input lag | Hidden damage. Any fault reverts at once |
| During a test | Dead-man timer: a trial setting reverts unless the system stays healthy for a set time | A frozen desktop that cannot ask for help |
| During a test | systemd watchdog on the daemon | A hung daemon leaving settings half-applied |
| Across reboots | All changes runtime-only; a setting becomes permanent only after days of healthy use | A bad setting surviving a reboot |
| Across reboots | An unclean shutdown during a trial blocks that setting on this machine | Repeating the freeze |
| Across reboots | Safe mode: three unclean boots in a row, a `dynamis.safe=1` boot option, or a file in `/etc` starts the daemon with stock settings | A boot loop the user cannot escape |
| Fleet-wide | Remote kill switch in the signed quirk feed disables a setting for an affected model within hours | One discovery protecting every user |
| Fleet-wide | New setting plugins ship to a beta channel first, and go stable only after enough clean reports | Untested settings reaching ordinary users |
| Fleet-wide | Shared results are a starting point only; each machine re-verifies them locally | Wrong or poisoned community data |
| Hard limits | Never voltage, firmware, BIOS, thermal trip points, fans below firmware minimum, filesystem barriers or charge protection | Physical damage and data loss |
| Battery | Running from battery while plugged in only above a charge floor and below a temperature limit; charge limits within firmware range | Battery wear and deep discharge |
| Software | Small root daemon in Rust, polkit for app actions, no network in the daemon, hardened systemd unit, fuzzed D-Bus input, signed releases | The daemon becoming a security hole |
| AI | No language model in the control loop; its suggestions are reviewed by a person | Confident wrong commands run as root |
| User | Undo on every change, "Restore original settings", a notice whenever a rollback happens, clean uninstall back to stock | Loss of trust |

## The change journal

Every change is written to the journal before it is applied: time, setting, old value, new value, reason, and whether it is a trial. The journal is what makes rollback, safe mode and diagnosis possible. If a laptop starts misbehaving, the journal shows exactly what was active at the time. Format: [decision 0007](decisions/0007-plain-file-change-journal.md).

## What is implemented today

Phase 2 has started. These rails exist in code (`daemon/crates/dynamis-core/src/engine.rs`, `writer.rs`, `settings/`) and are covered by tests in `daemon/crates/dynamis-core/tests/settings_engine.rs`:

| Rail | How |
| --- | --- |
| Probe first, within limits | Each plugin probes the machine and declares the values it accepts. Anything else is refused before the journal is touched. Some limits are tighter than the kernel's: CPU speed limit never below 30%, charge limit never below 50%, PCIe `powersupersave` not offered at all |
| One write path | Only the private `writer` module writes, only `Engine` calls it, and it refuses any path outside a fixed allow-list and never creates files |
| Journal before writing | `pending.json` and a `start` line are flushed to disk before the kernel is touched |
| Verify after writing | The value is read back; if it differs, the old value is written back and the change is journaled as `failed` |
| One change at a time | A lock file allows one process; a pending trial must be kept or reverted before the next change |
| Dead-man trial | `dynamis set … --trial N` undoes the change unless the user types `y` within N seconds. No answer, end of input or anything else means undo |
| Crash-aware rollback | If Dynamis Power starts and finds `pending.json`, it puts the old value back, journals `recover` and **blocks** the setting until `dynamis unblock` |
| Restore original settings | `baseline.json` keeps each setting's first-seen value, never overwritten. `dynamis restore` writes them all back |
| Safe mode | `/etc/dynamis-power/disable` or `dynamis.safe=1` on the kernel command line refuses every change. Restore still works |
| Risk tiers | Risky and Experimental settings need `--allow-risky`; Advisory settings are never applied |
| Runtime only | Plugins write only live kernel values under `/sys` and `/proc/sys`. Nothing is written to persistent configuration, so a reboot also undoes everything |

The service, `dynamisd`, adds:

| Rail | How |
| --- | --- |
| Recovery at boot | `dynamisd` opens the engine before anything else at start, so an unfinished change is undone and blocked at boot |
| No network | The systemd unit sets `PrivateNetwork=yes`, `IPAddressDeny=any` and allows only local sockets |
| Hardened service | Read-only filesystem except the state directory and the kernel settings it tunes, no new privileges, 64 MB memory cap, and more; see `daemon/dist/dynamisd.service` |
| Who may change profiles | polkit: the person at the laptop yes, remote or background sessions no. If polkit cannot be asked, the answer is no |
| One owner | Refuses to start if power-profiles-daemon, tuned-ppd or a second copy owns the interface, before touching any setting |
| Clean uninstall | `daemon/dist/uninstall.sh` stops the service, restores every original setting, removes its files and turns back on the service it replaced |

Phase 2 also adds:

| Rail | How |
| --- | --- |
| Health monitor | During a trial, checks every second for new kernel errors and known trouble (GPU hangs, lockups, storage timeouts, device resets, firmware crashes), memory or disk thrashing beyond where it was before the trial, and temperatures over 95 °C. Any of these undoes the change at once and blocks the setting. Code: `health.rs` |
| Quirk list and kill switch | A built-in list plus a signed, daily-downloaded list block known-bad settings per model. A newly blocked setting that is already applied is put back. The download runs as an unprivileged user; the root service only verifies and reads. See [decision 0009](decisions/0009-signed-quirk-list.md) |
| Safe mode after unclean shutdowns | Three boots in a row that follow a freeze, crash or power cut switch safe mode on. It stays on until a person runs `dynamis safe-mode off`, so a setting that causes freezes cannot cycle back on. Code: `boots.rs` |
| Bounded journal | The journal rotates at 1 MB, keeping one previous file |

Not built, by decision: the systemd watchdog, and the change rate limit until automatic changes exist ([decision 0010](decisions/0010-no-systemd-watchdog-yet.md)). Still to come: pre-flight checks (battery floor, no call or full-screen video) with automatic experiments in phase 4, and dropping capabilities to the minimum after the real-hardware test.
