# 0022: Which power tools run alongside, and which Dynamis Power offers to switch off

**Status:** Accepted

## Context

Many tools change the same settings. Two tools changing one setting fight (see the outside-change detection, everyday problem §4). Some tools touch things Dynamis Power never does, and can run alongside.

## Decision

**Run alongside**, with Dynamis Power leaving their settings alone while they run:

- **thermald**: handles heat with its own limits; Dynamis Power never touches thermal limits.
- **asusctl / asusd**: while running, Dynamis Power leaves the firmware power mode (platform profile) and the charge limit to it, and says so in Advanced.
- **supergfxctl**: switching between graphics chips is left to it; Dynamis Power only lets an unused second chip sleep, and not at all while supergfxctl is running.

**Offer to switch off** (with a yes, recorded so removing Dynamis Power turns them back on): TLP, auto-cpufreq, TuneD and tuned-ppd, power-profiles-daemon, Laptop Mode Tools, PowerTOP's auto-tune service, and **System76 Power**, **TUXEDO Control Center's** power daemon (tccd) and **Slimbook Battery**. These last three set the processor's speed preference, governor and frequency limits themselves, the same settings Dynamis Power is built around, so they cannot share the laptop with it.

## Consequences

- `conflicts.rs` adds TUXEDO and Slimbook to the list it can switch off; System76 Power is already there.
- Settings left to asusd are marked "managed by Asus Linux" in Advanced, and learning skips them.
