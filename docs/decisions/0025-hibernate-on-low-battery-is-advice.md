# 0025: Sleep that turns into hibernation is advice only

**Status:** Accepted

## Context

systemd can sleep and then hibernate when the battery runs low or after a while (suspend-then-hibernate), which stops a sleeping laptop from running flat. It needs swap at least as large as memory, the right kernel resume settings (often in the bootloader), and a desktop setting to use it. Getting any of these wrong can lose work or stop the laptop from waking.

## Decision

- Dynamis Power does not set it up. Setting it up touches the bootloader and persistent system configuration, which safety rules 3 and 5 forbid.
- It checks whether the laptop is ready (enough swap, resume configured) and explains in plain words what is missing and how to set it up, with a link to the distribution's guide. The sleep test's report says whether hibernation is available.

## Consequences

- People who want it do it themselves, knowingly. Dynamis Power stays runtime-only.
