# 0021: Every unclean shutdown counts towards safe mode, however long after the last change

**Status:** Accepted

## Context

Three boots in a row that follow a freeze, crash or power cut switch safe mode on. Some of those freezes happen days after Dynamis Power last changed anything, and are probably not its fault. Counting only freezes soon after a change would mean fewer false alarms.

## Decision

- Keep counting every unclean shutdown. Safe mode is the safe side: a setting can cause trouble long after it was applied (for example only when the laptop sleeps, or under a rare load).
- The notice already says what was changed in the ten minutes before each freeze, and, when nothing was, that the cause is probably elsewhere. Freezes that continue with safe mode on are counted apart, and then the notice says Dynamis Power is unlikely to be the cause.

## Consequences

- On a laptop with freezes unrelated to Dynamis Power, safe mode can switch on without reason; the notice explains it and switching it off is one step.
- No change to the code; this records why it stays as it is.
