# Design

This document explains how Dynamis Power decides what to do. For where the code lives, see [ARCHITECTURE.md](ARCHITECTURE.md).

## Core idea: a profile is a goal, not a list of settings

Other tools define "Power Saver" as a fixed list of values. Here a profile is:

- **an objective** — what to minimise or maximise: watts, stall time, temperature;
- **constraints** — limits it must respect, such as maximum stall or temperature;
- **bounds** — the range each setting may explore;
- **pins** — settings the user fixed, which are never changed;
- **app policy** — which apps may be throttled or frozen.

The values that meet the goal are learned per machine. Illustrative definition (values are placeholders):

```toml
[profile.power-saver]
objective = "min_watts"
max_stall_pct = 5        # share of time the focused app waits for CPU
max_temp_c = 80

[profile.power-saver.bounds]
"cpu.epp" = ["balance_power", "power"]
"cpu.turbo" = [false, true]
"cpu.max_perf_pct" = [50, 100]

[profile.power-saver.apps]
background = "throttle"
```

## Profiles

| Profile | Goal |
| --- | --- |
| Performance | Fastest response and throughput |
| Balanced (default) | Best efficiency with no noticeable slowdown |
| Power Saver | Longest battery; small slowdowns allowed |
| Extreme Saver | Reach a charger |
| Quiet and Cool | Low fan noise and surface temperature |
| Media | Smooth video for the fewest watts |
| Call and Presentation | Steady audio, video and screen; no experiments |
| Burst | Finish a heavy job fast, then return to the previous profile |
| Gaming | Stable frame rate |
| Docked | Desk use on mains |

The first three appear to the desktop through the standard power-profiles interface. Extended profiles map to the nearest standard one for apps that only know three.

**"Last until" mode:** the user sets a target ("5 hours", "until 6 pm"). Remaining battery energy becomes a watt budget, and the app tightens the least noticeable settings first.

**Overlays** combine with any profile: Battery Care (charge limit), adaptive charging, metered network, and "do not experiment".

## Auto mode in six layers

Each layer works without the ones after it.

1. **Context rules** — charger, battery level, video, calls, games, temperature, app requests. *(Built, phase 3.)*
2. **Workload detection** — eight classes: idle, light interactive, bursty interactive, video, call, sustained compute, gaming, background transfer. Rules first; a small classifier later. *(Rules built, phase 3.)*
3. **Calibration** — tests each setting alone on this machine. *(Built, phase 4: [decision 0012](decisions/0012-calibration-method.md).)*
4. **Online learning** — tunes settings that interact (mainly CPU) per workload class.
5. **Prediction** — habits, time remaining, pre-emptive moves.
6. **Shared results** — starts from what worked on the same model, then re-verifies locally.

## How learning works

- **Calibration:** alternate one setting on and off in short blocks (A B A B) during quiet periods; compare average power and stall time; store the effect with a confidence level. No measurable benefit means the safe default stays.
- **Optimisation:** for interacting settings, Bayesian optimisation or a contextual bandit over the stored experiment records. Explores only inside the profile's bounds, spends a small share of time exploring, reverts at once if responsiveness drops.
- **Responsiveness is a hard limit**, never a term traded against power. Published research on learned power managers found that the instantly measurable power reward tends to dominate the policy otherwise.
- **For compute jobs, score energy per job, not watts.** Throttling can make a job take longer and use more total energy.

## Learning on battery and plugged in

| Power source | Goal | Measured with |
| --- | --- | --- |
| Battery | Fewest watts within the stall limit | Battery discharge rate, CPU energy counters |
| Plugged in | Fastest stable speed, lowest heat and noise, battery health | Stall time, job time, temperatures and throttling, CPU energy counters, and — on supporting firmware — running from battery while plugged in |

See [MEASUREMENT.md](MEASUREMENT.md) for the details.

## Expected learning time

Rough estimates, to be measured in phase 4:

| Stage | Rough time |
| --- | --- |
| Known-safe settings applied | Minutes |
| Each setting calibrated | 5–15 hours of quiet battery time, spread over 1–2 weeks |
| Interacting settings tuned per activity | 2–6 weeks |
| Afterwards | Occasional rechecks after kernel updates or drift |

Shared results for the same model cut this to days. The app never claims an exact optimum: measurements are noisy and the target moves.

## Where AI fits

| Job | Technique | Runs |
| --- | --- | --- |
| Finding best settings | Bayesian optimisation or contextual bandit | On the laptop |
| Recognising workload | Rules, then a small decision tree | On the laptop |
| Forecasting | Per-weekday statistics, simple regression | On the laptop |
| Explaining and diagnosing | Language model reading the journal | Optional, opt-in; never in the control loop |
