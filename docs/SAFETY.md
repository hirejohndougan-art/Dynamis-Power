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
| Never fight another tool | A setting Dynamis Power set that reads differently on two readings in a row was changed by something else: journaled as `outside`, with the likely tool (a running program, a udev rule writing the same file, or a tool's service set to start), and not changed back. The second time in a day the setting is blocked, so Dynamis Power stops managing it (and stops keeping it). Code: `dynamisd/src/outside.rs`, `engine.rs` |
| Safe mode as a test | Settings changed in the ten minutes before each unclean shutdown (or in progress at the time) are noted as suspects. Unclean shutdowns while safe mode is on are counted apart: Dynamis Power changed nothing then. Switching safe mode off blocks the suspects only if no unclean shutdown happened while it was on; otherwise the cause is probably elsewhere and they are let go. Code: `boots.rs`, `engine.rs` |
| What was happening | After an unclean shutdown the service reads the end of the previous boot's kernel log (`journalctl -b -1 -k`, read-only) and its own journal, and Home says in plain words whether the laptop was going to sleep or waking, whether a device reported trouble, and what Dynamis Power had changed. The service unit allows reading the kernel log (`ProtectKernelLogs=no`) for this and for the trial health monitor. Code: `dynamisd/src/crash.rs` |
| Bounded journal | The journal rotates at 1 MB, keeping one previous file |

Phase 3 (auto mode) adds:

| Rail | How |
| --- | --- |
| Change rate limit | At most 12 automatic switches an hour, at least 60 s apart, with hysteresis on every threshold. Only urgent changes (charger, critical battery, a call, coming back to the laptop) skip it |
| People win | A profile picked by hand pauses auto mode until the charger is plugged in or out; app holds always win |
| Same safety path | Auto mode only switches between the three standard profiles, through the same engine, journal and quirk checks as any other change |
| Off by default | Auto mode does nothing until switched on |

Phase 4 (learning) adds:

| Rail | How |
| --- | --- |
| Pre-flight checks | Tests start only on battery at 40%+, below 80 °C, during idle or light use, with no app holds, no other trial and no safe mode |
| Stop when things change | Charger, workload, profile or an app request ends a test early; the original value is put back |
| Responsiveness is a hard limit | A change that adds more than 0.5 points of stall is rejected however much power it saves |
| Health monitor during tests | Trouble undoes the test at once and blocks the setting |
| Off means off | Switching learning off ends any test and puts learned settings back |
| Owner-aware recovery | A pending change is undone after a crash only when the process that made it is gone, so a running test is never undone by mistake |

Phase 5 (tuning together) adds:

| Rail | How |
| --- | --- |
| Still one change at a time | Settings that affect each other are tuned by single steps; no trial ever changes two settings at once ([decision 0014](decisions/0014-tuning-together-one-step-at-a-time.md)) |
| Lag limit for the whole set | The stall added by every kept step together may not exceed the single-step limit, so small steps cannot add up to a slow laptop |
| Bounded search | One grid step at a time, inside the setting's own limits (speed limit never below 30%; the grid stops at 50%), at most 12 kept steps per power mode |
| Per power mode | Results only count for the power mode they were measured under; another mode starts from the original values |

Kept settings add:

| Rail | How |
| --- | --- |
| Not system configuration | Settings set by hand (the battery's limits always, others when "Keep after restart" is ticked) are listed in the service's own state and applied again at startup and after waking, through the engine. Nothing is written to persistent system files |
| Probation | A kept setting is trusted after an hour in force. After an unclean shutdown, settings still on probation are not applied again, so a setting that freezes the laptop cannot return by itself at every boot |
| Safe mode and dry run | Kept settings are not applied in either; Restore stops keeping anything |

Settings toward replacing TLP add:

| Rail | How |
| --- | --- |
| One file per new directory | The writer accepts under `/sys/block` only `queue/scheduler`, under `/sys/class/drm` only the AMD power level, and under `/sys/power` only `mem_sleep` (never `state`, which would put the laptop to sleep) |
| Limits on the new settings | Charge start 40–95% and below the charge limit; top frequency never below 30% of the fastest; governors never `userspace`; AMD power level only `auto`, `low`, `high` |
| Sleep type needs a yes | `system.mem_sleep` is Risky: some firmware wakes badly from one kind of sleep |

Phase 7 (prediction) adds:

| Rail | How |
| --- | --- |
| Only existing levers | "Last until" caps the power mode and, at its strongest, slows background apps. It never touches brightness, never freezes apps, never changes a setting outside the usual engine path |
| Ends by itself | Plugging in, reaching the time, or *Restore original settings* ends it and lifts the cap at once |
| Gentle steps | One level at a time, at least five minutes apart |
| Charge limit only, 80–100% | Charging that follows habits only moves the existing charge-limit setting between 80% and 100%, through the engine (journaled), off by default, only where the battery offers the control. Off, or Restore, puts the original limit back. Without a clear habit it charges fully, as normal |

Phase 6 (apps) adds:

| Rail | How |
| --- | --- |
| Shares, never caps | Only an app's `cpu.weight` changes (normal 100, in front 400, slowed 25). A weight decides who goes first when apps compete; no app is ever capped, paused or frozen ([decision 0013](decisions/0013-app-shares-not-caps.md)) |
| Only user apps | The writer accepts only `cpu.weight` under `/sys/fs/cgroup/user.slice/`; the systemd unit keeps the rest of the cgroup tree read-only. System services and the desktop itself are never touched |
| Off by default | Counting is on; nothing is slowed or boosted until switched on. Background slowing needs battery power and a known app in front |
| Same journal, same undo | Every change is journaled and shown in Activity. Restore puts every share back and turns the switches off; a restart of the service puts back anything left over. Safe mode refuses changes |

Advanced adds:

| Rail | How |
| --- | --- |
| Profile choices within the probe | A person can pick what each mode sets, only for that mode's own settings and only among the values this laptop offers; anything else is refused, and an outdated file is ignored when planning |
| Dry run | A file in the state directory makes every change journal `would` and write nothing; trials are refused, learning waits, restore still writes |
| Advisor writes nothing | TLP and TuneD output is text for the person; the service never writes configuration files |

Other power tools can be switched off from the app's welcome only after an administrator password, only from a fixed list (TLP, auto-cpufreq, Laptop Mode Tools, TuneD, System76 Power, PowerTOP auto-tune, power-profiles-daemon, tuned-ppd), and each one is recorded so removing Dynamis Power turns it back on.

Not built, by decision: the systemd watchdog ([decision 0010](decisions/0010-no-systemd-watchdog-yet.md)). Still to come: dropping capabilities to the minimum after the real-hardware test.
