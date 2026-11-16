# 0016: Modes use turbo, the speed limit and the governor where there is no speed preference

**Status:** Accepted

## Context

A mode (Longer battery, Balanced, Faster) changed only two things: the processor's speed preference (EPP) and the firmware power mode (platform profile), and a mode was offered only if it would change something. Budget and older processors often have neither; on the project's own test laptop (Celeron N4020) only Balanced was offered, so the mode choice, per-app modes and the panel icon showed one option.

## Decision

- When a laptop has neither control, modes use **fallback levers**, each through its own probed setting plugin and the engine:

  | Mode | Turbo | Speed limit / top frequency | Governor |
  | --- | --- | --- | --- |
  | Longer battery | off | 60% (or the lowest listed frequency at least 60% of the top) | a calm one: `powersave`, `schedutil`, `ondemand` or `conservative` |
  | Balanced | on | 100% | the same calm one |
  | Faster | on | 100% | `performance` |

  Every mode sets every lever, so switching always undoes the previous mode. Where a laptop has the usual controls, they are used and the fallback is not.
- A mode is still offered only when it changes something; when one is not, the app says why ("this laptop has no controls for it", or "it would be the same as Balanced").
- **Modes and learning agree.** Learning tunes turbo and the speed limit (decision 0014). On fallback laptops the mode is part of what each result records, so tuning is per mode; when nothing is learned for the mode in use, or on mains, the values go back to what the mode sets, not to values from before tuning. Forgetting does the same.
- Advanced → Profiles offers the fallback levers for each mode.

## Consequences

- Longer battery saves real power on such laptops (turbo is the largest lever there); Faster can mean more heat and fan on laptops that have one.
- The 60% figure is a starting point; learning can tune it per mode on battery, and a person can change it in Profiles.
- The mapping is checked on a test copy of the synthetic laptop with its speed preference removed; the VivoBook's real files are still to be captured as a fixture.
