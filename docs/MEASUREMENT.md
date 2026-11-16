# Measurement

Measurement is the hardest part of the project. Battery readings are slow and noisy, the display dominates total power, and no workload repeats exactly. Everything the learner decides rests on this document, so changes to measurement deserve the most careful review.

## Signals

| Signal | Source | Used for |
| --- | --- | --- |
| Whole-system power | `/sys/class/power_supply/BAT*/power_now`, or current × voltage | Battery-life decisions (on battery, or plugged in with `charge_behaviour` = `force-discharge`) |
| CPU and GPU energy | RAPL counters in `/sys/class/powercap` | CPU-side tests, on battery or mains |
| Stall time | Pressure stall information: `/proc/pressure/{cpu,memory,io}` and per-cgroup `*.pressure` | The main responsiveness measure |
| Temperature and throttling | `/sys/class/thermal`, hwmon, `thermal_throttle` counters | Guardrails; sustained-speed learning |
| Idle depth | `cpuidle` residency, platform low-power residency, GPU RC6 | Finding what keeps the machine awake |
| Context | UPower, logind, PipeWire, MPRIS, NetworkManager, compositor | Deciding when a test is valid |

Pressure stall information supports threshold triggers via `poll()`, so the daemon can sleep until stalls appear. See the [kernel documentation](https://docs.kernel.org/accounting/psi.html).

## Techniques

| Problem | Technique |
| --- | --- |
| Whole-system power only visible on battery | Battery discharge on battery; RAPL on mains; force-discharge where firmware allows |
| The display swamps other effects | Learn a brightness-to-watts curve once and subtract it |
| Workload varies | Test in idle windows, or compare within one workload class |
| Slow drift from heat and battery state | Interleave short blocks (A B A B) instead of one long A then B |
| Noisy gauge | Require a minimum effect size and confidence before keeping a change |
| Small effects | Accumulate evidence across sessions |
| The user changes something mid-test | Discard any block where the context changed |
| A saving that costs responsiveness | Record stall time in the same blocks; reject changes over the profile's limit |
| Throttling that makes jobs run longer | Score energy per job, not watts, for compute work |

## Battery gauges differ

Some gauges update every few seconds, others far less often; some report charge instead of power. On first run the app measures the gauge's update rate and resolution and sizes test blocks to match.

## Plugged-in measurement

On mains the goal is speed, heat and noise: stall time, app launch time, job duration, sustained frequency before throttling, and fan speed. CPU energy comes from RAPL. On laptops whose firmware supports `charge_behaviour` = `force-discharge`, the app can briefly run from the battery while plugged in to measure whole-system power — only above a charge floor and below a temperature limit.

## Experiment records

Each test produces a record: setting ID, value, baseline value, context (power source, workload class), metric, effect with confidence interval, sample count, verdict (keep, reject, inconclusive), and any fault. These records are the app's model of the machine, and they are exactly what shared results contain. See [SHARING.md](SHARING.md).
