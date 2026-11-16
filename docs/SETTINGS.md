# Settings catalogue

Every lever the app can tune. Each row becomes one setting plugin (see [ARCHITECTURE.md](ARCHITECTURE.md#plugins)).

**Status:** planned unless marked otherwise. Interface paths are typical and vary by driver and kernel; every plugin must probe for its path and report "unsupported" when absent. Paths below are from general kernel knowledge and must be verified on real hardware when each plugin is written.

**Implemented so far (phase 2):** `cpu.epp`, `cpu.turbo`, `cpu.max_perf_pct`, `platform.profile`, `vm.swappiness`, `vm.dirty_writeback`, `kernel.nmi_watchdog`, `audio.power_save`, `pcie.aspm`, `battery.charge_limit`. See the [implemented plugins](#implemented-plugins) section for the exact values each accepts. `dynamis settings` lists them on a real machine.

**Setting IDs are permanent.** They appear in journals, learned data and shared results. Add new IDs; never rename or reuse old ones. To retire one, mark it deprecated here.

## CPU

| ID | What it controls | Typical interface | Risk |
| --- | --- | --- | --- |
| `cpu.epp` | How eagerly the CPU raises its speed | `/sys/devices/system/cpu/cpufreq/policy*/energy_performance_preference` | Safe |
| `cpu.governor` | Frequency governor | `.../policy*/scaling_governor` | Safe |
| `cpu.max_perf_pct` | Upper speed limit | `/sys/devices/system/cpu/intel_pstate/max_perf_pct` | Safe |
| `cpu.max_freq` | Top frequency, for drivers without a speed limit in percent | `.../policy*/scaling_max_freq`, from `scaling_available_frequencies` | Safe |
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
| `gpu.amd_power_level` | AMD graphics power level (`auto`, `low`, `high`) | `/sys/class/drm/card*/device/power_dpm_force_performance_level` | Usually safe |
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

`usb.autosuspend` and `pci.runtime_pm` are not setting plugins: devices come and go, and each has its own original value. They are two switches on the Battery screen (`SetDevicePolicy`, `dynamisd/src/devices.rs`), applied per device by the engine's `set_device_power` (originals in `devices.json`, journaled as `device-power`, undone by Restore) on battery and not in Faster, and to devices plugged in later at the next reading. The ids can be blocked per laptop like any setting. Never touched: USB input (HID), audio, printers, still image (phones, cameras), communications (modems, mobile broadband) and wireless controllers (Bluetooth), by class or by driver (`usbhid`, `btusb`, `snd-usb-audio`, `usblp`, `qmi_wwan`, `cdc_mbim`, `cdc_ncm`, `cdc_acm`, `cdc_wdm`, `option`, `sierra`, `huawei_cdc_ncm`); PCI graphics (by class and by driver) and the Intel management engine (`mei_me`). Code: `dynamis-core/src/devices.rs`.
| `audio.power_save` | Sound chip sleeps when silent | `/sys/module/snd_hda_intel/parameters/power_save` | Usually safe |
| `audio.controller_power` | Sound controller sleeps too | `/sys/module/snd_hda_intel/parameters/power_save_controller` | Usually safe |

## Kernel and system

| ID | What it controls | Typical interface | Risk |
| --- | --- | --- | --- |
| `kernel.nmi_watchdog` | Periodic NMI watchdog wakeups | `/proc/sys/kernel/nmi_watchdog` | Safe |
| `sched.scx` | sched_ext scheduler per profile | scx loader | Experimental |
| `system.mem_sleep` | Sleep type (`s2idle` or `deep`) | `/sys/power/mem_sleep` | Risky (some firmware wakes badly from one kind) |
| `system.defer_maintenance` | Defer updates, indexing, backups to mains | systemd runtime drop-ins (`ConditionACPower=`) | Safe |

## Apps (cgroup v2)

| ID | What it controls | Interface | Risk |
| --- | --- | --- | --- |
| `app.foreground_boost` **(built)** | The app in front gets a bigger share of the CPU | `cpu.weight` | Safe |
| `app.background_throttle` **(built)** | Apps not in front get a smaller share on battery | `cpu.weight` | Safe |
| `app.background_freeze` | Hidden apps paused at low battery | `cgroup.freeze` | Not planned ([decision 0013](decisions/0013-app-shares-not-caps.md)) |

These are not setting plugins: they are switches on the Apps screen (`SetAppPolicy`), applied per app group under `user.slice` by the engine's `set_app_weight`, journaled as `app-weight` and undone by restore. `cpu.max`, `cpu.uclamp.max` and `io.weight` are not used.

## Battery

| ID | What it controls | Interface | Risk |
| --- | --- | --- | --- |
| `battery.charge_limit` | Stop charging at a set level | `/sys/class/power_supply/BAT*/charge_control_end_threshold` | Safe where supported |
| `battery.charge_start` | Start charging only below a level | `/sys/class/power_supply/BAT*/charge_control_start_threshold` | Safe where supported |
| `battery.charge_behaviour` | Run from battery while plugged in, for measurement | `/sys/class/power_supply/BAT*/charge_behaviour` | Usually safe, with floor and temperature limits |

## Implemented plugins

Code: `daemon/crates/dynamis-core/src/settings/plugins.rs`. Values are what people type and what `dynamis settings` shows; the plugin translates them to kernel values.

| ID | Values | Notes |
| --- | --- | --- |
| `cpu.epp` | The CPU's own list, minus `default` | Written to every CPU policy. Refused while the governor is `performance`, which locks it. Refused when CPUs disagree, so there is one value to return to |
| `cpu.turbo` | `on`, `off` | `intel_pstate/no_turbo` (inverted) or `cpufreq/boost` |
| `cpu.max_perf_pct` | `min_perf_pct` (but at least 30) to 100 | Intel only |
| `platform.profile` | The firmware's own list | |
| `vm.swappiness` | 0–200 | |
| `vm.dirty_writeback` | 100–6000 centiseconds (1–60 s) | Usually safe: longer means more unsaved data in a crash |
| `kernel.nmi_watchdog` | `on`, `off` | |
| `audio.power_save` | 0–60 seconds | Usually safe: may pop on some hardware |
| `pcie.aspm` | `default`, `performance`, `powersave` | `powersupersave` deliberately not offered |
| `cpu.min_perf_pct` | 0 to the current speed limit | Intel only |
| `cpu.max_freq` | The driver's own frequency list, in MHz, down to 30% of the top | Only where there is no speed limit in percent and the driver lists frequencies, so the value reads back exactly |
| `cpu.governor` | The driver's governors, minus `userspace` | Written to every CPU policy. Used by modes only on laptops without a speed preference |
| `cpu.hwp_dynamic_boost` | `on`, `off` | Intel HWP |
| `audio.controller_power` | `on`, `off` | Usually safe |
| `storage.io_scheduler` | The schedulers every real disk offers | Applied to every real disk (not loop, RAM, zram, device-mapper, RAID or optical) |
| `gpu.amd_power_level` | `auto`, `low`, `high` | Every AMD card. Manual and profiling levels not offered |
| `battery.charge_start` | 40–95%, below the charge limit | Refused at or above the limit |
| `system.mem_sleep` | `s2idle`, `deep` | Risky: needs an explicit yes. Offered only where both exist. Takes effect at the next sleep |
| `battery.charge_limit` | 50–100% | First battery with `charge_control_end_threshold` |

### Adding a plugin

1. Pick a permanent ID from the tables above (or add a new row).
2. Implement `Setting` in `plugins.rs`: probe, read, plan. `plan` returns the writes; it must not write.
3. Add its path prefix to `ALLOWED_PREFIXES` in `writer.rs` if it is new. Keep that list short.
4. Add it to `all()`, add fixture files, and test it on a fixture copy in `tests/settings_engine.rs`.
5. Update the table here. A test fails if the ID is missing from this file.

## Never

Voltage and undervolting, overclocking, firmware and BIOS, bootloader, thermal trip points, fan speed below firmware minimum, filesystem barriers, battery charge protection. See [SAFETY.md](SAFETY.md).
