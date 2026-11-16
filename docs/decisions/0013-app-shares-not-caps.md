# 0013: Apps get a bigger or smaller share, never a cap or a freeze

**Status:** Accepted

## Context

Phase 6 makes Dynamis Power app-aware: how much battery each app uses, priority for the app in front, and slowing background apps on battery. Linux offers several cgroup v2 controls for this: `cpu.weight` (a share when apps compete), `cpu.max` and `cpu.uclamp.max` (hard limits), and `cgroup.freeze` (stops every process in the group).

Hard limits and freezing save more, but they can break things a person does not see: a download stalls, a sync client misses its window, a call in a background tab drops, an app that is frozen while holding a lock blocks another. Such breakage is hard to trace back to a power tool.

## Decision

- Only `cpu.weight` is changed, and only in app groups under `user.slice`. A weight is a share, not a cap: an app with a small share still gets every bit of processor time nobody else wants, so nothing stops working. The writer refuses every other cgroup file and every path outside `user.slice`; the systemd unit makes the rest of the cgroup tree read-only.
- Three shares: normal (100), the app in front (400), slowed (25).
- Nothing changes by default. Slowing background apps applies only on battery, and only when the app in front is known (otherwise every app would count as background).
- Every change is journaled, shown in Activity, and put back by *Restore original settings*, which also turns the switches off. Safe mode refuses changes.
- Energy per app is an **estimate**: power above the laptop's idle level, shared out by processor time, on battery only. The app says so.
- The session agent learns the app in front from a tiny KWin script and sends only a process ID. The service turns it into an app name on the same computer.

`cpu.max`, `cpu.uclamp.max`, `io.weight` and `cgroup.freeze` stay unused. Revisit freezing only with an explicit opt-in, per app, and a measured benefit.

## Consequences

- Savings are smaller than with caps or freezing; they show up when apps compete, which is when the app in front would otherwise wait.
- Focus tracking works on KDE Plasma first. GNOME has no supported way for a script to report focus; there, background slowing waits and the Apps screen says so.
- Reading `/proc` for every process costs a little on every reading (every 15 s, every minute while idle). Counting can be switched off in the app.
