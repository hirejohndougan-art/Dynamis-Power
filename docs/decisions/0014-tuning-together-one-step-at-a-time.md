# 0014: Tune interacting settings together, one step at a time

**Status:** Accepted

## Context

Phase 5 tunes processor settings whose effects depend on each other: turbo boost and the speed limit, under the speed preference and firmware mode that the power mode sets. Turning turbo off can add lag at full speed yet cost nothing once the speed limit is lower. Testing each setting once from wherever the others happen to be misses combinations like that.

DESIGN.md suggested Bayesian optimisation or a contextual bandit. Both pick the next point to try from a model fitted to earlier results, and usually move several settings at once. On a laptop the search space is small: two settings with eight useful values between them, per power mode. Each test takes about 15 minutes of quiet battery time, so there are few data points. A model that is wrong in a way nobody notices also makes the result hard to explain.

## Decision

- **Coordinate descent with re-testing.** From the current values, try one step of one setting (turbo the other way; the speed limit 10% up or down). Each step is an ordinary phase 4 experiment: one change, a trial, alternating blocks, kept only if it saves power with confidence and adds no lag.
- **Results are tied to where they were measured.** Each record stores the values of the other tuned settings and of the power mode's settings. When one setting moves, the steps for the others are tried again from the new point. Results for one power mode never apply to another.
- **Lag is limited for the whole set.** The total stall added by the kept steps (each counted as at least zero) may not exceed the single-step limit. Small steps that each add a little lag are rejected once they add up.
- **One change at a time stays.** No trial ever changes two settings at once, so the journal, trial, health monitor and crash recovery work exactly as before. A combination a single step cannot reach from the current values is not tried.
- **It ends.** Every kept step lowers measured power by at least the minimum effect, so it cannot cycle. A cap of 12 kept steps per power mode is a backstop.
- Learned values apply on battery for the power mode they were found under, and the original values return on mains or under a power mode with nothing learned. Forgetting either setting forgets the set.

## Consequences

- Fewer combinations are explored than a joint optimiser would try; in exchange each step is measured, explainable and individually safe.
- The search is per power mode, so a laptop that stays in Balanced is tuned for Balanced first.
- Earlier single results for turbo and the speed limit (format version 1) are dropped on upgrade and re-tested together.
- A model-based optimiser can still be added later on top of the same records, if real laptops show the simple search leaves savings behind.
