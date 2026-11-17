# 0027: Acting through the desktop

**Status:** Accepted

## Context

Some of the largest remaining savings are not kernel settings but desktop ones: a fast screen's refresh rate, file indexing, the dark style on OLED screens, and how soon the screen dims. The service runs as root and cannot see inside a session; the session agent (`dynamis-agent`) runs as the person and already talks to the desktop.

These are the person's own settings, not system configuration, so safety rule 3 (nothing written to persistent system configuration) does not cover them directly, but the same spirit applies.

## Decision

1. **The agent acts, the service decides.** The service says what it wants (`desktop` in `Status`: lower refresh rate, pause indexing, dark style), from the charger and the person's choices; the agent applies it through the desktop's own interfaces and puts it back.
2. **Temporary where the desktop allows it, put back where it does not.** GNOME's display configuration is applied as temporary. KDE's `kscreen-doctor` and the dark style are saved by the desktop, so the agent records what it changed and puts the person's value back when plugged in, when the rule is switched off, when the agent stops, and at the next login if it was killed.
3. **Off unless chosen, except indexing.** Pausing file indexing on battery is on by default: it is fully reversible and the indexer catches up when plugged in. Refresh rate and dark style are offered as switches; the app suggests them where they help (a screen above 60 Hz; an OLED screen).
4. **Screen dimming stays advice.** (Changed by [0029](0029-screen-page-timeout-and-animated-dim.md): the person can now set the screen-off time on the Screen page, and animated dim fades the screen before it turns off.) Changing the person's dim and off times would replace their choice with ours; the app says when it would help and where to change it.
5. **The person wins.** If the person changes the refresh rate or style by hand while a rule is in force, the agent leaves it until the next charger change.

**Built.** Service: `dynamisd/src/desktop.rs` (`SetDesktopPolicy`, `desktop` in `Status`, `Desktop()` and `ReportDesktop` for the agent); the OLED guess from the screen's EDID text (`dynamis-core/src/edid.rs`: Samsung Display `ATNA` part numbers, or "OLED" in the name). Agent: `dynamis-agent/src/desktop.rs`, acting only when what the service wants changes. Baloo is paused only if the person had not suspended it; GNOME's indexer resumes by itself if the agent leaves the bus. What the agent changed is recorded in `~/.local/state/dynamis-power/desktop.json` before acting and put back when the agent is stopped (logout, or `systemctl --user stop`: SIGTERM, SIGINT and SIGHUP are caught with `signal-hook`), and at its next start if it was killed outright. Bluetooth off when unused is a radio rule in the service (`bluetooth_off_when_unused`), reading open connections from `/sys/class/bluetooth`, so it needs no desktop. All of it awaits a real KDE and GNOME desktop.

Both indexers already hold back on battery on their own (Baloo stops content indexing; GNOME's indexer is set not to index on battery by default), so pausing them saves less than first thought: mainly the basic indexing and file watching after many files change.

## Consequences

- Savings TLP cannot reach, on the two most common desktops. Other desktops get the kernel-side savings only.
- More moving parts in the agent, each needing a real desktop to confirm (part of the hardware tests).
