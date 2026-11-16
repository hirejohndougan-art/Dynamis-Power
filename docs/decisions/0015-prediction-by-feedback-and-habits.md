# 0015: "Last until" steers by feedback; habits are simple and kept in UTC

**Status:** Accepted

## Context

Phase 7 adds a time target ("make the battery last until 6 pm"), a battery forecast from habits, and charging that follows when the laptop is usually unplugged. The design called for turning the remaining energy into a watt budget and tightening the least noticeable settings first.

The service has no time zone database and no clock library, and it must stay small, offline and explainable.

## Decision

- **"Last until" is feedback, not a model.** Budget = (energy left − a 5% reserve) ÷ hours left. A ten-minute average of power use is compared with it. Above budget, it saves one level harder; below 75% of budget, it eases off one level. It waits five minutes between moves. Levels, each including the last: no full speed → Longer battery → Longer battery plus slowing background apps. It uses only the existing, already-safe levers (the power mode cap and app shares). It caps app requests too, because the target is the person's own explicit goal. It ends by itself when the charger is plugged in or the time is reached, and *Restore original settings* ends it.
- **Habits are two small things:** typical power on battery for each of the 168 hours of the week (a slow moving average), and the last 60 unplug times. They are stored on this computer only and say nothing about what the laptop was used for.
- **Times are kept in UTC.** A daily routine is still a routine at the same UTC hour. Daylight saving shifts it by an hour twice a year, which the 45-minute agreement test and the 2.5-hour charging margin absorb. The app shows local times.
- **A usual unplug time** needs at least three unplugs on that weekday in the last six weeks that agree within about 45 minutes (median distance from the median). Otherwise there is no habit, and charging is normal.
- **Charging that follows habits** changes only the existing charge-limit setting, between 80% and 100%, through the engine. It is off by default and offered only where the battery has the control. Switching it off puts the original limit back.

## Consequences

- No calibrated "watts per level" model is needed, and every move is explainable from two numbers shown in the app (budget and recent use).
- At its strongest level "Last until" may still fall short; the app says so rather than taking riskier steps (no brightness, no freezing apps).
- Forecasts start rough and sharpen over a week or two of use.
