# 0012. How calibration tests and keeps a setting

- **Status:** Accepted
- **Date:** 2026-11-16

## Context

Phase 4 tests settings one at a time on each laptop and keeps only what measurably helps (DESIGN.md, MEASUREMENT.md). Battery gauges are noisy and slow, the battery drifts as it warms and drains, and a saving that costs responsiveness is not a saving.

## Decision

- **Alternating blocks.** Each experiment runs eight equal blocks, A B A B A B A B (A = current value, B = candidate). Each B block is compared with the average of the A blocks on either side, which cancels slow drift. Readings in the first seconds of a block are ignored while the machine settles. Default: 60 s blocks, so one experiment takes 8 minutes.
- **Keep** only when power falls by at least 0.05 W and 1% of baseline, with t ≥ 2.5 across the four paired differences, **and** stall (time tasks wait for CPU, memory or disk) rises by no more than 0.5 percentage points. Stall is a hard limit, never traded against watts. **Reject** when stall rose too much or even the optimistic end of the 95% interval saves too little. Otherwise **inconclusive**: the current value stays, and it is retried later (three tries at most).
- **Pre-flight.** Tests run only on battery, at 40% or more, below 80 °C, during idle or light use, with no app holds, no other trial, and not in safe mode. They stop early if the charger is plugged in, the workload changes, the profile changes, an app asks for a profile, or the health monitor sees trouble (that last case rejects the setting and blocks it).
- **Same safety path.** The candidate is applied as a trial through the engine, so it is journaled, quirk-checked, and undone after a crash.
- **Learned settings apply on battery only**, because that is where they were measured. On mains they go back to their original values. Switching learning off also puts them back. `dynamis learned forget SETTING` undoes one.
- **Scope.** Only settings outside the standard profiles: `cpu.epp` and `platform.profile` interact with them and with each other and are left to phase 5. Risky and experimental settings are never tested automatically.
- **Pending changes record their owner** (PID, process start time, boot ID). Recovery undoes a pending change only when its owner is gone, so the service can reopen the engine at every step of an experiment, and other `dynamis` commands can run meanwhile.
- **Display model.** Brightness and power are fitted passively during quiet battery use. Nothing is changed to learn it.

## Why

On a simulated laptop with known effects, 0.3 W of noise and 0.4 W of drift, this kept a real 0.3 W saving in 200 of 200 runs and wrongly kept a useless change in 4 of 200. End to end, it recovered every built-in effect to within 0.2 percentage points of stall and 0.01 W.

## Consequences

- One pass over the current candidates takes roughly two to three hours of quiet battery time, spread over days.
- A false keep (about 2%) costs nothing dangerous: the setting is safe by tier, and a later test or `forget` removes it.
- Records use the shared-results field names (`schema/v1`), so sharing them later needs no conversion.
- Phase 4's gate, beating stock Balanced on battery, can only be judged on real hardware (docs/TESTING-ON-HARDWARE.md).
