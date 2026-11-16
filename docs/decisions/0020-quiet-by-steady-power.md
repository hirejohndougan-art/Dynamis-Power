# 0020: Learning judges "quiet" by steady power, with a processor limit scaled by the number of cores

**Status:** Accepted

## Context

Learning tests a setting only while the laptop is quiet, so that the measurement is not drowned by changing work. Quiet has meant low processor use. A slow two-core laptop running a browser and a desktop is rarely under that limit, so learning hardly runs there, though power may be perfectly steady.

## Decision

- A moment is quiet when **power use has been steady for a minute**: the spread of the battery's readings over that minute is small compared with their average (the exact limit is set from the battery's measured noise, see everyday problem §2).
- The processor limit stays as a second check, but it scales with the number of cores: a busy share that is normal for one core on a two-core chip does not count as busy.
- Where power cannot be read often enough to judge steadiness, the processor check alone is used, as before.

## Consequences

- Learning runs on slow laptops in everyday light use.
- A steady but heavy workload is still excluded by the processor check and by the workload class.
- The thresholds are starting points; real-hardware results may tune them.
