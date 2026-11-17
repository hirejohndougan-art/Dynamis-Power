# 0026: Learning per activity

**Status:** Accepted, with learning during heavy work as an option the person can switch on

## Context

Learning tests at quiet moments on battery, so what it keeps is what saves power during idle and light use. The same learned values are then applied whatever the person is doing. That is right for most settings: device sleep, Wi-Fi, the sound chip and the disk behave the same during a video as during reading. It is not right for the settings whose cost is speed: turbo, the processor speed limits and speed preference (tuned together in phase 5), and the graphics speed limits. A speed limit that is free while reading can stutter a video or slow a compile.

Results already record the workload they were measured in (`context.workload`), so the data format does not need to change.

## Decision

1. **Three activities.** Everyday use (idle and light use together: tests already move freely between the two, so their results cannot be told apart), video (a video playing, including full screen), and heavy work (sustained computing). Calls and games are left out: auto mode already gives them their own mode, and learning does not touch them.
2. **Only speed-related settings are learned per activity**: the phase 5 group (turbo, speed limits, speed preference) and the graphics speed limits. Every other setting keeps one result for all activities, as now.
3. **What is tested where.**
   - Idle and light use: as now.
   - Video: tested only during a steady video lasting long enough for a whole test. A test whose activity changes part-way is discarded, as now. Graphics speed is tested during video too, judged by whether the graphics chip keeps up (below).
   - Heavy work: **not tested by default**, because power and lag vary too much to measure fairly. During heavy work the speed-related settings use the power mode's own values, not learned ones, so learning never slows real work. The person can switch on *Also learn during heavy work* (Learning screen): then the speed settings tuned together are tested during steady heavy work and its own results are applied during it.
4. **Applying.** When the activity changes and the new one has lasted at least a minute, the speed-related learned values switch to that activity's results. An activity with no result yet uses the mode's own values for those settings; heavy work does too unless learning during it is on. Each switch is journaled like any learned change and can be undone.
5. **Existing results** count as everyday use.
6. **Progress.** The Learning screen says how many video tests have run, and has the switch for heavy work. "Learn now" stays idle plus a light load.

**Graphics keeping up (added after acceptance).** Counting dropped frames needs each app or the compositor to report them, and there is no common way to ask. Instead, a graphics speed test's "lag" is the share of readings in which the graphics chip was fully busy (at least 95%, no idle gaps between frames) while at its speed limit: a chip that keeps up with a video has idle time between frames. Intel: RC6 idle residency and the actual frequency; AMD: `gpu_busy_percent` (`dynamis-core/src/sensors/gpu.rs`). Where neither exists, graphics speed is not tested during video. This is a proxy, not a frame count; checking it against real stutter is part of the hardware tests.

Built in `dynamis-core/src/activity.rs` and `dynamisd/src/learn.rs` (`learned_for`, `speed_records`, `observe_activity`); `SetLearnDuringHeavyWork`; `learning.heavy`, `learning.activity` and `learning.activity_tests` in `Status`.

## Consequences

- No slowdown during heavy work from learned speed limits. Today such limits can apply then.
- Video gets its own processor and graphics results over time; graphics only where the chip's busy time can be read.
- More tests in total, so learning takes longer to finish. Video tests only happen during long videos.
- Graphics speed per activity: learned for everyday use and video; heavy work keeps the driver's own.
