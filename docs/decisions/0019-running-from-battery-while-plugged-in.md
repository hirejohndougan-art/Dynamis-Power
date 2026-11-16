# 0019: Running from the battery while plugged in: opt-in, standard control only, recalibration first

**Status:** Accepted

## Context

Some laptops can be told to run from the battery with the charger connected (`charge_behaviour` = `force-discharge` in `/sys/class/power_supply/BAT*/`). That would let learning measure at a desk, and it is what battery recalibration needs (run the battery down, then charge it fully, so the gauge learns its real capacity). On some laptops the firmware stays in that state until the charger is unplugged, or the setting survives a reboot.

## Decision

- Used only where the kernel offers `force-discharge` in the standard file. No vendor-specific tools, no guessing.
- Off unless the person turns it on, with an explicit yes each time; the app says plainly that the battery will run down while plugged in.
- At first it is used only for **battery recalibration** on request. Learning while plugged in comes later, after it has been seen to work on real laptops.
- Always switched back to `auto` at the end, when stopped, at the next service start and after waking from sleep. It also stops at 15% battery, if the laptop gets warm, in safe mode, or if the charger is unplugged.
- Never used for battery charge protection (safety rule 5 is unchanged).

## Consequences

- Recalibration (TLP's `recalibrate`) becomes possible on laptops with the standard control; others are told it is not available here.
- If a laptop stays stuck in discharge, unplugging and plugging back in, or a restart, ends it; the testing guide asks people to check this.
