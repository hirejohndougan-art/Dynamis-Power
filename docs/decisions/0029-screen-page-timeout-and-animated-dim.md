# 0029. The Screen page: screen-off time and animated dim

- **Status:** Accepted
- **Date:** 2026-11-16
- **Changes:** point 4 of [0027](0027-acting-through-the-desktop.md) ("screen dimming stays advice")

## Context

The owner asked for everything about the screen in one place in the main menu, for the screen-off time to be set there, and for a gentler way of turning the screen off: fading down over the last 15 seconds and brightening quickly on return ("animated dim"), with a switch to turn the fading off.

Decision 0027 kept screen dimming as advice, because changing the person's dim and off times would replace their choice with ours. Here the person makes the choice themselves, on our page: we write what they pick, nothing else.

## Decision

1. **A Screen page in the main menu** holds the brightness now, the brightness for each mode (moved from Modes), the screen-off time, animated dim, and the refresh rate and dark style on battery (moved from Devices).
2. **Screen-off time, on battery and plugged in**, from 30 seconds to an hour, or "your desktop's own setting" (the default). It is written to the desktop's own settings by the session agent, so the desktop keeps turning the screen off itself: KDE Plasma's `powerdevilrc` (AC, Battery and LowBattery), GNOME's `idle-delay` (one value, switched with the charger). What was there before is recorded first and put back when the person picks the desktop's own setting again, at Restore, and when the agent stops (or at its next start if it was killed).
3. **Animated dim, on by default.** Fifteen seconds before the screen-off time the brightness fades, gently at first, from where it is to 10% of the range; the desktop then turns the screen off. Any use during the fade puts the brightness straight back. When the person comes back, it starts at 10% and returns to where it was in about a third of a second. The desktop's own dimming is switched off while animated dim is on (recorded and put back like the times), so the two do not fight. Nothing fades while a video or presentation keeps the screen on.
4. **How.** Brightness is set through logind's `SetBrightness` on the person's own session, as brightness keys do: no root, nothing journaled (it is the person's own brightness, not tuning). Idle time comes from the desktop (`GetSessionIdleTime` on KDE, Mutter's idle monitor on GNOME). The fade runs in its own thread in the agent, asleep until the fade is due; it reads the idle time every quarter second during the fade and once a second while the screen is off.
5. **Safe mode** does not stop these: they are the desktop's settings, chosen by the person, not changes Dynamis Power makes to the system.

## Consequences

- Screen dimming is no longer advice only: the *screen uses a lot of the battery* tip now points to the Screen page.
- More agent behaviour that needs a real KDE and GNOME desktop to confirm: the config keys, PowerDevil picking up the change, the idle time calls, and how brightness behaves when the screen comes back on.
- Brightness changes during the fade are visible to the desktop as ordinary brightness changes; its slider moves with them and returns with them.
