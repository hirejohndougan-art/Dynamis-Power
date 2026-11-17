# Hardware coverage: maker-specific controls

Every power-related control that laptop makers' kernel drivers offer, and what Dynamis Power does with each. Step 1 of "catch up on maker-specific support" in [ROADMAP.md](../ROADMAP.md).

**Source:** the kernel's own documentation and drivers in the mainline tree as of 16 November 2026 (commit `666539e`): `Documentation/ABI/testing/sysfs-platform-*`, `sysfs-driver-*`, `sysfs-class-power`, `sysfs-class-platform-profile`, `Documentation/admin-guide/laptops/`, `Documentation/wmi/devices/`, and `drivers/platform/x86/`, `drivers/platform/surface/`, `drivers/power/supply/cros_charge-control.c`. Facts only; no code or text from other tools. Controls that are not about power (camera switches, Fn lock, touchpad toggles, RGB lighting) are left out.

**Status:**

- **Supported**: Dynamis Power uses it today.
- **To add**: allowed by the safety rules and worth a setting plugin; step 2 of the roadmap.
- **Needs a decision**: the safety rules do not settle it clearly; a decision record comes first.
- **Not allowed**: covered by safety rule 5 (firmware or BIOS settings, overclocking, fans below the firmware minimum, battery protection).

**How common** is a rough order from public laptop sales, not a measurement: the first group covers most laptops sold, the last is rare.

## Controls every maker can use (standard kernel interfaces)

| Control | Interface | Status |
| --- | --- | --- |
| Firmware power mode | `/sys/firmware/acpi/platform_profile`; since 6.14 also one `/sys/class/platform-profile/platform-profile-N/` per handler | Supported (`platform.profile`, through the combined file). Separate handlers (for example a laptop driver and AMD PMF both offering modes) are set together by the kernel; reading them one by one is a possible improvement |
| Charge limit | `charge_control_end_threshold` on the battery | Supported (`battery.charge_limit`) |
| Charge start threshold | `charge_control_start_threshold` | Supported (`battery.charge_start`) |
| Charge types (Standard, Long Life, Adaptive, Custom, Fast) | `charge_types` | **To add**: a `battery.charge_type` setting offering only Standard, Long Life, Adaptive and Custom; Fast and Express wear the battery faster and are left to the person. Several drivers now use this instead of their own files (Lenovo IdeaPad, Acer, Dell, Uniwill/Tuxedo, Lenovo gaming laptops) |
| Charge behaviour (inhibit charge, force discharge) | `charge_behaviour` | Supported for force discharge (recalibration, decision 0019). Inhibit charge: **needs a decision** (see HP below) |

## By maker

### Most common

**Lenovo**

| Driver | Control | Status |
| --- | --- | --- |
| `thinkpad_acpi` (ThinkPad) | Charge start and end thresholds, charge behaviour | Supported (standard files) |
| `thinkpad_acpi` | Firmware power mode | Supported |
| `thinkpad_acpi` | Keyboard light (`tpacpi::kbd_backlight`) | Supported (off on battery) |
| `thinkpad_acpi` | Fan control | Not allowed below the firmware minimum; not used at all |
| `ideapad-laptop` (IdeaPad, Yoga, Legion) | Battery conservation (`conservation_mode`) | Supported, but the kernel now marks this file deprecated in favour of `charge_types` (Long Life). **To add:** use `charge_types` where present, keep `conservation_mode` for older kernels |
| `ideapad-laptop` | `fan_mode` (quiet, standard, dust cleaning, efficient thermal dissipation) | **Needs a decision** (maker fan modes). The firmware power mode usually covers it |
| `ideapad-laptop` | `usb_charging` (USB ports charge devices while the laptop is off) | **To add** (see "USB charging while off or asleep") |
| `ideapad-laptop` | Firmware power mode | Supported |
| `lenovo-wmi-gamezone` (Legion, LOQ) | Firmware power mode | Supported |
| `lenovo-wmi-other` (Legion, LOQ) | Processor power limits (`ppt_pl1_spl`, `ppt_pl2_sppt`, `ppt_pl3_fppt`, `ppt_pl4_ipl`, `ppt_pl1_tau`, `ppt_cpu_cl`), graphics boost (`gpu_nv_cpu_boost`, `dgpu_boost_clk`) | **Needs a decision** (processor power limits) |
| `lenovo-wmi-other` | Charge types, charge behaviour | Charge types: to add (standard); behaviour: as above |
| `think-lmi` | BIOS settings | Not allowed |

**HP**

| Driver | Control | Status |
| --- | --- | --- |
| `hp-wmi` | Firmware power mode | Supported |
| `hp-wmi` | Charge behaviour (auto, inhibit charge, force discharge) | HP laptops have **no charge limit file**; holding the charge with "inhibit charge" at a level would give them one. **Needs a decision**: it is battery handling the firmware does not offer itself, and decision 0019 allows only force discharge today |
| `hp-bioscfg` | BIOS settings | Not allowed |

**Dell**

| Driver | Control | Status |
| --- | --- | --- |
| `dell-laptop` | Charge start and end thresholds | Supported (standard files) |
| `dell-laptop` | Charge types (Standard, Adaptive, Custom, Express, Primarily AC) | To add (`battery.charge_type`; Express not offered) |
| `dell-laptop` | Keyboard light (`dell::kbd_backlight`), its own timeout (`stop_timeout`) and ambient-light control | Light: supported (off on battery). Timeout: **to add** (a shorter timeout on battery) |
| `dell-pc` | Firmware power mode | Supported |
| `alienware-wmi` | Firmware power mode; fan boost (`fanY_boost`) | Power mode: supported. Fan boost only raises fans, costs power: not used |
| `dell-wmi-sysman` | BIOS settings | Not allowed |

**ASUS**

| Driver | Control | Status |
| --- | --- | --- |
| `asus-wmi` | Charge limit | Supported (standard file) |
| `asus-wmi` | Firmware power mode (`throttle_thermal_policy` and `fan_boost_mode` on older models) | Supported through `platform_profile` |
| `asus-wmi` | `mcu_powersave` (ROG Ally and some laptops: the keyboard and controller chip sleeps with the laptop) | **To add**: on is the maker's own lower-power choice. The kernel notes it should always be on with fixed firmware; a quirk covers older firmware |
| `asus-wmi` | `panel_od` (screen overdrive), `mini_led_mode` | **Needs a decision** (panel controls that change how the screen looks); Mini LED mode may save real power |
| `asus-wmi`, `asus-armoury` | Processor power limits (`ppt_pl1_spl`, `ppt_pl2_sppt`, `ppt_pl3_fppt`, `ppt_apu_sppt`, `ppt_platform_sppt`), `nv_dynamic_boost`, `nv_temp_target` | **Needs a decision** (processor power limits). Lowering them is not overclocking, but they are firmware-level limits |
| `asus-wmi` | `dgpu_disable`, `gpu_mux_mode`, `egpu_enable` | GPU switch needs a reboot (firmware setting): not allowed. Turning the second graphics chip off at runtime: **needs a decision** (apps using it crash) |
| `asus-wmi` | `charge_mode` (read-only: how it is charging) | Read only; useful for diagnostics |
| `asus-wmi` | Custom fan curves | Not allowed (could go below the firmware minimum) |

**Acer**

| Driver | Control | Status |
| --- | --- | --- |
| `acer-wmi` | Firmware power mode (Predator, Nitro and others) | Supported |
| `acer-wmi-battery` | Charge types: Standard or Long Life (stops at 80%) | To add (`battery.charge_type`) |
| `acer-wmi` | Fan modes | **Needs a decision** (maker fan modes) |

### Common

**MSI**

| Driver | Control | Status |
| --- | --- | --- |
| `msi-ec` | Charge start and end thresholds | Supported (standard files). MSI firmware ties the start to 10 points below the limit; a quirk should say so |
| `msi-ec` | Fan mode, shift (performance) mode | **Needs a decision** (maker fan and performance modes) |
| `msi-laptop` (older models) | `eco_mode`, `turbo_mode`, `auto_fan` | Eco mode: **to add** (older models only). Turbo: not used |
| `msi-wmi-platform` | Fan and power readings | Read only |

**Samsung**

| Driver | Control | Status |
| --- | --- | --- |
| `samsung-galaxybook` | Charge limit, firmware power mode | Supported (standard files) |
| `samsung-galaxybook` | USB charging while off | **To add** (see below) |
| `samsung-laptop` (older models) | Battery life extender | Supported |
| `samsung-laptop` | `performance_level` (silent, normal, overclock) | Silent only: **needs a decision** (maker modes); overclock: not allowed |
| `samsung-laptop` | `usb_charge` | **To add** (see below) |

**Microsoft Surface**

| Driver | Control | Status |
| --- | --- | --- |
| `surface_platform_profile` | Firmware power mode | Supported |

**Framework and Chromebooks**

| Driver | Control | Status |
| --- | --- | --- |
| `cros_charge-control` | Charge start and end thresholds, charge behaviour | Supported (standard files) |
| `wilco_ec` (older Dell Chromebooks) | Thresholds, charge type, `usb_charge` | Thresholds supported; charge type to add; USB charging to add |

**Huawei and Honor**

| Driver | Control | Status |
| --- | --- | --- |
| `huawei-wmi` | Charge thresholds (own file and standard files) | Supported |

### Less common

| Maker | Driver | Control | Status |
| --- | --- | --- | --- |
| LG | `lg-laptop` | Battery care limit, charge limit | Supported |
| LG | `lg-laptop` | `fan_mode`, `usb_charge` | Fan: needs a decision; USB charging: to add |
| Toshiba / Dynabook | `toshiba_acpi` | Charge limit | Supported (standard file; 80 or 100) |
| Toshiba / Dynabook | `toshiba_acpi` | `usb_sleep_charge`, `sleep_functions_on_battery`, `usb_rapid_charge` | Sleep charging on battery: **to add** (off on battery); rapid charge: not used |
| Toshiba / Dynabook | `toshiba_acpi` | Keyboard light timeout | To add, like Dell's |
| Fujitsu | `fujitsu-laptop` | Charge limit | Supported (standard file) |
| System76 | `system76_acpi` | Charge thresholds | Supported (standard files) |
| Tuxedo, Uniwill, XMG and other Clevo-like | `uniwill-acpi` | Charge limit, charge types | Limit supported; charge types to add |
| Tuxedo, Uniwill | `uniwill-acpi` | `ctgp_offset` (graphics power), `usb_powershare_high`, `usb_c_power_priority` | Graphics power: needs a decision; the rest: not used |
| Sony (older) | `sony-laptop` | Battery care limiter | Supported |
| Panasonic | `panasonic-laptop` | `eco_mode` (charges to about 80%) | **To add** |
| Gigabyte, Xiaomi, Redmi, Inspur, Dasharo | various | Sensors, hotkeys, firmware power mode | Power mode supported where offered; nothing else power-related |
| Handhelds (OneXPlayer, AYANEO) | `oxpec`, `ayaneo-ec` | Controller power, turbo toggle | Not laptops; not covered |

## Gaps in one place

**To add** (step 2 of the roadmap, most common first):

1. `battery.charge_type` from the standard `charge_types` file (Standard, Long Life, Adaptive, Custom). Covers Lenovo IdeaPad and Legion (and replaces the deprecated `conservation_mode` route), Acer, Dell, Uniwill/Tuxedo, older Dell Chromebooks.
2. **USB charging while off or asleep** off on battery: Lenovo `usb_charging`, Samsung `usb_charge` (both drivers), LG `usb_charge`, Toshiba `usb_sleep_charge`/`sleep_functions_on_battery`, Wilco `usb_charge`. Saves battery while asleep; a plain switch on Devices.
3. ASUS `mcu_powersave` on.
4. Keyboard light timeout on battery: Dell `stop_timeout`, Toshiba `kbd_backlight_timeout`.
5. Panasonic `eco_mode`, older MSI `eco_mode`.
6. Read each `/sys/class/platform-profile/` handler separately for clearer reporting.

**Needs a decision** (one record for all of them, before any is added):

- Processor and graphics power limits through maker drivers: ASUS `ppt_*`, `nv_dynamic_boost`, `nv_temp_target`; Lenovo gaming `ppt_*`; Uniwill `ctgp_offset`.
- Maker fan and performance modes outside the firmware power mode: Lenovo `fan_mode`, Acer and MSI fan and shift modes, LG `fan_mode`, Samsung `performance_level` (silent).
- Screen modes: ASUS `panel_od` and `mini_led_mode`.
- Turning a second graphics chip off at runtime (ASUS `dgpu_disable`).
- A charge limit by "inhibit charge" for laptops without one (HP).

**Not allowed** (safety rule 5): BIOS settings (`hp-bioscfg`, `dell-wmi-sysman`, `think-lmi`, and firmware attributes generally), GPU switch modes that need a reboot, custom fan curves, manual fan control, overclock modes, fast or express charging chosen for the person.

## Keeping this current

The monthly check (roadmap step 5) compares the kernel's documentation and drivers with this list and opens an issue for anything new. Each row's status changes in the same pull request as the code.
