# Settings catalogue

Every lever the app can tune. Each row becomes one setting plugin (see [ARCHITECTURE.md](ARCHITECTURE.md#plugins)).

**Status:** planned unless marked otherwise. Interface paths are typical and vary by driver and kernel; every plugin must probe for its path and report "unsupported" when absent. Paths below are from general kernel knowledge and must be verified on real hardware when each plugin is written.

**Setting IDs are permanent.** They appear in journals, learned data and shared results. Add new IDs; never rename or reuse old ones. To retire one, mark it deprecated here.

## CPU

| ID | What it controls | Typical interface | Risk |
| --- | --- | --- | --- |
| `cpu.epp` | How eagerly the CPU raises its speed | `/sys/devices/system/cpu/cpufreq/policy*/energy_performance_preference` | Safe |
| `cpu.governor` | Frequency governor | `.../policy*/scaling_governor` | Safe |
| `cpu.max_perf_pct` | Upper speed limit | `/sys/devices/system/cpu/intel_pstate/max_perf_pct` or `scaling_max_freq` | Safe |
| `cpu.min_perf_pct` | Lower speed limit | `/sys/devices/system/cpu/intel_pstate/min_perf_pct` | Safe |
| `cpu.turbo` | Turbo boost | `/sys/devices/system/cpu/intel_pstate/no_turbo` or `cpufreq/boost` | Safe |
| `cpu.hwp_dynamic_boost` | Intel HWP dynamic boost | `/sys/devices/system/cpu/intel_pstate/hwp_dynamic_boost` | Safe |
| `cpu.rapl_long` / `cpu.rapl_short` | Sustained and burst watt limits | `/sys/class/powercap/intel-rapl:*/constraint_*_power_limit_uw` | Usually safe |
| `cpu.idle_latency` | How deeply the CPU may sleep | `/dev/cpu_dma_latency`; per-CPU `power/pm_qos_resume_latency_us` | Safe |
| `platform.profile` | Firmware quiet/balanced/performance mode | `/sys/firmware/acpi/platform_profile` | Safe |

Some firmware expects either `platform.profile` or `cpu.epp`, not both. Each plugin must know which path a machine expects.

## Display and graphics

| ID | What it controls | Typical interface | Risk |
| --- | --- | --- | --- |
| `display.brightness` | Backlight level, within user limits | `/sys/class/backlight/*/brightness` | Safe |
| `display.refresh_rate` | Panel refresh rate | Compositor / DRM | Usually safe |
| `display.psr` | Panel self-refresh | Boot parameter | Advisory |
| `gpu.max_freq` | Integrated GPU frequency ceiling | Driver sysfs | Usually safe |
| `gpu.dgpu_runtime_pm` | Discrete GPU power-down | PCI `power/control` | Usually safe |
| `kbd.backlight` | Keyboard backlight | `/sys/class/leds/*kbd_backlight*/brightness` | Safe |

## Storage

| ID | What it controls | Typical interface | Risk |
| --- | --- | --- | --- |
| `storage.sata_alpm` | SATA link power | `/sys/class/scsi_host/host*/link_power_management_policy` | Medium usually safe; minimum power risky |
| `storage.nvme_apst` | NVMe low-power states | Boot parameter | Advisory |
| `storage.io_scheduler` | I/O scheduler | `/sys/block/*/queue/scheduler` | Safe |
| `storage.readahead` | Read-ahead size | `/sys/block/*/queue/read_ahead_kb` | Safe |
| `vm.dirty_writeback` | Write-back batching | `/proc/sys/vm/dirty_writeback_centisecs` | Usually safe |

## Memory

| ID | What it controls | Typical interface | Risk |
| --- | --- | --- | --- |
| `vm.swappiness` | Swap eagerness | `/proc/sys/vm/swappiness` | Safe |
| `vm.zram_size` / `vm.zram_algo` | Compressed RAM swap | `/sys/block/zram*/disksize`, `comp_algorithm` | Safe |
| `vm.thp` | Transparent huge pages | `/sys/kernel/mm/transparent_hugepage/enabled` | Safe |

## Radios, buses and peripherals

| ID | What it controls | Typical interface | Risk |
| --- | --- | --- | --- |
| `wifi.power_save` | Wi-Fi power save | nl80211 | Usually safe |
| `bt.power` | Bluetooth off when unused | rfkill | Safe |
| `pcie.aspm` | PCIe link power | `/sys/module/pcie_aspm/parameters/policy` | Usually safe if firmware enables it; forcing is risky |
| `pci.runtime_pm` | Idle PCI devices power down | Per-device `power/control` | Usually safe |
| `usb.autosuspend` | Idle USB devices sleep (input and audio excluded) | Per-device `power/control` | Usually safe |
| `audio.power_save` | Sound chip sleeps when silent | `/sys/module/snd_hda_intel/parameters/power_save` | Usually safe |

## Kernel and system

| ID | What it controls | Typical interface | Risk |
| --- | --- | --- | --- |
| `kernel.nmi_watchdog` | Periodic NMI watchdog wakeups | `/proc/sys/kernel/nmi_watchdog` | Safe |
| `sched.scx` | sched_ext scheduler per profile | scx loader | Experimental |
| `system.defer_maintenance` | Defer updates, indexing, backups to mains | systemd runtime drop-ins (`ConditionACPower=`) | Safe |

## Apps (cgroup v2)

| ID | What it controls | Interface | Risk |
| --- | --- | --- | --- |
| `app.foreground_boost` | Focused app gets more CPU and I/O | `cpu.weight`, `io.weight` | Safe |
| `app.background_throttle` | Background apps clamped on battery | `cpu.weight`, `cpu.uclamp.max`, `cpu.max` | Usually safe |
| `app.background_freeze` | Hidden apps paused at low battery | `cgroup.freeze` | Risky (opt-in) |

## Battery

| ID | What it controls | Interface | Risk |
| --- | --- | --- | --- |
| `battery.charge_limit` | Stop charging at a set level | `/sys/class/power_supply/BAT*/charge_control_end_threshold` | Safe where supported |
| `battery.charge_behaviour` | Run from battery while plugged in, for measurement | `/sys/class/power_supply/BAT*/charge_behaviour` | Usually safe, with floor and temperature limits |

## Never

Voltage and undervolting, overclocking, firmware and BIOS, bootloader, thermal trip points, fan speed below firmware minimum, filesystem barriers, battery charge protection. See [SAFETY.md](SAFETY.md).
