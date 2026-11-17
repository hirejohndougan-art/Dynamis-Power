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
| `cpu.min_freq` | Lowest frequency, for drivers without performance levels | `.../policy*/scaling_min_freq`, from `scaling_available_frequencies` | Safe |
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
| `gpu.max_freq` | Intel graphics top speed (TLP's `INTEL_GPU_MAX_FREQ`) | `/sys/class/drm/cardN/gt_max_freq_mhz` (i915), in 50 MHz steps between `gt_RPn_freq_mhz` and `gt_RP0_freq_mhz`; read back rounded to 50 MHz, since the driver rounds to its own steps (to confirm on real hardware). Never below the lowest speed now | Usually safe |
| `gpu.min_freq` | Intel graphics lowest speed (TLP's `INTEL_GPU_MIN_FREQ`) | `/sys/class/drm/cardN/gt_min_freq_mhz`, the same steps; never above the top speed now | Usually safe |
| `gpu.dgpu_runtime_pm` | A second graphics chip sleeps when idle (TLP's runtime PM for the discrete GPU) | PCI `power/control` of display controllers with `boot_vga` = 0 (never the chip driving the screen). Learning tests `on`; left to supergfxctl while it runs | Usually safe |
| `gpu.amd_power_level` | AMD graphics power level (`auto`, `low`, `high`) | `/sys/class/drm/card*/device/power_dpm_force_performance_level` | Usually safe |
| `display.amd_panel_power_savings` | AMD screen colour power saving (adaptive backlight modulation), 0 off to 4 strongest; Longer battery mode uses 3, as power-profiles-daemon does. Not learned: its cost is how colours look | `/sys/class/drm/card*-eDP-*/amdgpu/panel_power_savings` | Usually safe |
| `kbd.backlight` | Keyboard backlight | `/sys/class/leds/*kbd_backlight*/brightness` | Safe |

## Storage

| ID | What it controls | Typical interface | Risk |
| --- | --- | --- | --- |
| `storage.nvme_power_states` | NVMe drives' own idle power states (APST): `on` gives drives held off (latency tolerance 0) the kernel's default tolerance back; `off` holds them off. Learning tests `on`. Not offered where the kernel command line sets `nvme_core.default_ps_max_latency_us` | `/sys/class/nvme/nvme*/power/pm_qos_latency_tolerance_us` | Usually safe |
| `storage.sata_alpm` | SATA link power | `/sys/class/scsi_host/host*/link_power_management_policy` | Medium usually safe; minimum power risky |
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
| `wifi.power_save` | Wi-Fi power save | nl80211 netlink (`NL80211_CMD_GET/SET_POWER_SAVE`), every wireless interface whose driver reports it; [decision 0018](decisions/0018-wifi-power-saving-through-the-kernel-only.md). Learning tests `on`. Never on RTL8821CE or RTL8723BE (built-in quirks). NetworkManager may set it back when it reconnects; the outside-change rule then leaves it to NetworkManager | Usually safe |
| `net.wake_on_lan` | Whether wired network cards may wake the laptop from sleep (TLP's `WOL_DISABLE`) | `/sys/class/net/<card>/device/power/wakeup` (`enabled`/`disabled`), every wired card; Wi-Fi cards are left alone. Not tested by learning (it matters only during sleep); TLP's `WOL_DISABLE=Y` imports as off | Safe |
| `storage.disk_apm` | Spinning hard disks' power management level (TLP's `DISK_APM_LEVEL`) | ATA commands, as `hdparm -B`: `HDIO_GET_IDENTITY` (words 83, 86, 91) to read, `HDIO_DRIVE_CMD` SET FEATURES 0x05/0x85 to set; the writer takes `ata-apm:<disk>`. Only disks with `queue/rotational` = 1 that report APM. Offered: 128, 192, 254, `off`; never below 128 (the disk would spin down by itself and park its heads often). The spin-down timer (`hdparm -S`) cannot be read back, so it is not offered. Experimental until confirmed on real disks | Experimental |
| `bt.power` | Bluetooth off when unused | rfkill | Safe |
| `pcie.aspm` | PCIe link power | `/sys/module/pcie_aspm/parameters/policy` | Usually safe if firmware enables it; forcing is risky |
| `pci.runtime_pm` | Idle PCI devices power down | Per-device `power/control` | Usually safe |
| `usb.autosuspend` | Idle USB devices sleep (input and audio excluded) | Per-device `power/control` | Usually safe |

Radios (TLP's `DEVICES_TO_DISABLE_ON_BAT`, `_ON_LAN_CONNECT`, `_ON_STARTUP`) are not setting plugins either: rules on the Devices screen (`SetRadioPolicy`, `dynamisd/src/radios.rs`) switch rfkill software switches (`/sys/class/rfkill/rfkillN/soft`, the only rfkill file the writer allows) through the engine's `set_radio` (states before kept in `radios.json`, journaled as `radio`, undone by Restore; the id `radios` can be blocked). Kinds: `bluetooth`, `wlan`, `wwan`. Rules act only when their situation changes (charger, cable, dock, startup). Docked means an ACPI dock station reports `docked` (`/sys/devices/platform/dock.N/docked`) or a Thunderbolt device calls itself a dock (TLP's `DEVICES_TO_DISABLE_ON_DOCK`).

`usb.autosuspend` and `pci.runtime_pm` are not setting plugins: devices come and go, and each has its own original value. They are two switches on the Battery screen (`SetDevicePolicy`, `dynamisd/src/devices.rs`), applied per device by the engine's `set_device_power` (originals in `devices.json`, journaled as `device-power`, undone by Restore) on battery and not in Faster, and to devices plugged in later at the next reading. The ids can be blocked per laptop like any setting. The person can also keep a device model awake (`never` in the policy, `usb:vendor:product` or `pci:vendor:device`, so it holds in any port). Never touched: USB input (HID), audio, printers, still image (phones, cameras), communications (modems, mobile broadband) and wireless controllers (Bluetooth), by class or by driver (`usbhid`, `btusb`, `snd-usb-audio`, `usblp`, `qmi_wwan`, `cdc_mbim`, `cdc_ncm`, `cdc_acm`, `cdc_wdm`, `option`, `sierra`, `huawei_cdc_ncm`); PCI graphics (by class and by driver) and the Intel management engine (`mei_me`). Code: `dynamis-core/src/devices.rs`.
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
| `battery.charge_limit` | Stop charging at a set level | `/sys/class/power_supply/BAT*/charge_control_end_threshold` | Safe where supported Where the standard `charge_control_end_threshold` is missing, a laptop maker's own control is used (TLP's vendor charge controls), each found by its own file: Lenovo IdeaPad `ideapad_acpi/*/conservation_mode` (offered as 60 or 100), LG `lg-laptop/battery_care_limit` (80/100), Samsung `samsung/battery_life_extender` (80/100), Sony `sony-laptop/battery_care_limiter` (50/80/100), Huawei `huawei-wmi/charge_control_thresholds` (start 10 below the limit). The writer allows exactly those files. |
| `battery.charge_start` | Start charging only below a level | `/sys/class/power_supply/BAT*/charge_control_start_threshold` | Safe where supported |
| `battery.charge_behaviour` | Run from battery while plugged in: built for **battery recalibration** only ([decision 0019](decisions/0019-running-from-battery-while-plugged-in.md)), not a setting plugin. Engine `set_charge_behaviour` (`auto` or `force-discharge`, only values the file lists), used by `dynamisd/src/recal.rs`; always back to `auto` at the end, on any stop and at the next service start. Learning while plugged in is not built | `/sys/class/power_supply/BAT*/charge_behaviour` | Usually safe, with a 15% floor and temperature limit |

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
| `cpu.min_freq` | The driver's own frequency list, in MHz, up to the current top frequency | Offered where `cpu.max_freq` is |
| `storage.sata_alpm` | `max_performance`, `medium_power`, `med_power_with_dipm` | Every SATA host with the control. `min_power` deliberately not offered (some drives hang or lose data). Learning tests `med_power_with_dipm` |
| `cpu.governor` | The driver's governors, minus `userspace` | Written to every CPU policy. Used by modes only on laptops without a speed preference |
| `cpu.hwp_dynamic_boost` | `on`, `off` | Intel HWP |
| `audio.controller_power` | `on`, `off` | Usually safe |
| `storage.io_scheduler` | The schedulers every real disk offers | Applied to every real disk (not loop, RAM, zram, device-mapper, RAID or optical) |
| `gpu.amd_power_level` | `auto`, `low`, `high` | Every AMD card. Manual and profiling levels not offered |
| `storage.nvme_power_states` | `on`, `off` | Every NVMe drive. `on` writes the kernel's default tolerance (100000 µs) only; it never picks a deeper one |
| `display.amd_panel_power_savings` | 0–4 | Every AMD built-in screen. Set by Longer battery mode, not learned |
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
