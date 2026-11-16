# synthetic-laptop

**Source: synthetic.** Values are invented to be plausible, not captured from a real machine.

A minimal two-core Intel laptop running on battery:

- Identity: "Example Computers Inc." "Synthetic Laptop 14", chassis type 10 (notebook)
- CPU: two cpufreq policies on `intel_pstate` with HWP, turbo on, EPP `balance_performance`, 1100 MHz
- Memory: about 3.7 GB usable (rounds to 4 GB); zram swap of about 1.9 GB using zstd
- Storage: 62.5 GB eMMC (`mmcblk0`) plus a boot partition that must be ignored
- Wi-Fi: `wlan0` on the `rtw_8821ce` driver
- Battery `BAT0`: discharging, 5.2 W draw, 79% charge, 38 of 42 Wh design (90.5% health), 212 cycles
- Charger `AC0`: unplugged
- Thermal zones at 52 °C and 45 °C
- RAPL `package-0` energy counter with a 6 W sustained limit
- Backlight at half brightness
- Pressure stall files for CPU, memory and I/O
- No platform profile, no charge limit and no `charge_behaviour` (to test "not offered")
