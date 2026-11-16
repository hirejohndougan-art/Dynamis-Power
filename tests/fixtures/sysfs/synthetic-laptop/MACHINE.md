# synthetic-laptop

**Source: synthetic.** Values are invented to be plausible, not captured from a real machine.

A minimal two-core Intel laptop running on battery:

- CPU: two cpufreq policies on `intel_pstate`, turbo on, EPP `balance_performance`
- Battery `BAT0`: discharging, about 5.2 W draw, 79% charge, about 90% of design capacity
- Charger `AC0`: unplugged
- Backlight at half brightness
- Pressure stall files for CPU, memory and I/O
- `vm.swappiness` at 60

Use it for basic sensor and setting plugin tests.
