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

Every change is written to the journal before it is applied: time, setting, old value, new value, reason, and whether it is a trial. The journal is what makes rollback, safe mode and diagnosis possible. If a laptop starts misbehaving, the journal shows exactly what was active at the time.
