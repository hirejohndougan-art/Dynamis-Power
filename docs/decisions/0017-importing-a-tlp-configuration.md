# 0017: Importing a TLP configuration maps battery values to Longer battery and mains values to Faster

**Status:** Accepted

## Context

TLP sets most values twice, `..._ON_AC` and `..._ON_BAT`, and switches between them when the charger is plugged in or out. Dynamis Power sets values per mode (Longer battery, Balanced, Faster), and auto mode picks the mode from the charger and the workload. People moving from TLP want their tuning kept.

## Decision

- Read `/etc/tlp.conf` and `/etc/tlp.d/*.conf` (in TLP's own order; later files win). Text only: nothing in them is run.
- A value that is the same on mains and on battery (or has no `_ON_AC`/`_ON_BAT` form, such as the charge thresholds) becomes an ordinary setting, kept after restarts like a value set by hand in Advanced.
- An `_ON_BAT` value goes into **Longer battery**; an `_ON_AC` value goes into **Faster** (Advanced → Profiles overrides). **Balanced** keeps its defaults.
- Only values that map to a Dynamis Power setting and pass its probe (exists here, within the driver's limits, allowed by the risk tier) are offered. Values with no match are listed as "not imported" with the reason.
- The app shows the whole mapping first, line by line, with the original TLP line next to each. Nothing is applied until the person says yes, and each line can be unticked. Every applied value goes through the engine and is journaled.

## Consequences

- Someone who tuned TLP for battery gets that tuning in Longer battery; auto mode's "on battery" choice decides when it is used. If they want it in Balanced too, they choose that in Profiles.
- TLP's switching purely by charger is not reproduced exactly; the preview says so.
- Values outside what Dynamis Power allows (for example `min_power` SATA link power) are not imported, even if TLP set them.
