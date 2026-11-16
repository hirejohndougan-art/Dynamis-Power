# Testing on real hardware

This is the phase 1 test: it checks that Dynamis Power reads a real laptop correctly and costs almost nothing to run. **Every step only reads.** Nothing on the laptop is changed.

It takes about 1½ hours, most of it the laptop running normally on battery.

## The quick way: one command

After building (step 1 below), from the `daemon` folder:

```sh
sudo ./scripts/phase1-test.sh
```

It runs steps 2 to 6 for you, asks you to unplug the charger when needed, logs for an hour (`sudo ./scripts/phase1-test.sh 120` for two), prints a PASS/FAIL checklist, and saves everything in one `phase1-results-….tar.gz` to attach to a [Hardware report issue](https://github.com/hirejohndougan-art/dynamis-power/issues/new?template=hardware_report.yml). Ctrl+C during the log stops it early and still produces the report.

The log is forced onto the disk after every reading, so if the laptop freezes during the test, the readings up to the freeze survive. Run the script again after rebooting to get a fresh run; the old folder is kept.

The steps below are the same test done by hand.

## 1. Build

Follow "Building" in [../daemon/README.md](../daemon/README.md). Then, from the `daemon` folder:

```sh
alias dynamis="$PWD/target/release/dynamis"
```

## 2. Inventory (1 minute)

```sh
sudo dynamis inventory
```

Check it against what you know about the laptop: model, processor, memory, storage type and size, battery. Anything wrong or "unknown" is worth reporting.

## 3. Hardware report (1 minute)

```sh
sudo dynamis hardware-report > hardware-report.md
```

Open a [Hardware report issue](https://github.com/hirejohndougan-art/dynamis-power/issues/new?template=hardware_report.yml) and paste the file's contents. It contains no serial numbers, usernames or network names, but read it before posting.

## 4. Battery gauge (5 minutes, on battery)

Unplug the charger, then:

```sh
sudo dynamis gauge --duration 300
```

This shows how often the battery reports its power use, which decides how long each future test needs to be.

## 5. Power log (1 hour or more, on battery)

Still unplugged, start the log and use the laptop normally — browsing, documents, a video:

```sh
sudo dynamis log --sync -o power.csv
```

Stop with Ctrl+C when done. Then:

```sh
dynamis report power.csv
```

## 6. Check the results

| Check | Passes when |
| --- | --- |
| Whole-system power | Plausible for the laptop (a small laptop is usually 3–10 W) and changes when you work harder |
| Battery estimate | Roughly matches what the desktop's battery indicator says |
| CPU power | Shows numbers (needs `sudo`); "not recorded" means the CPU has no readable energy counters |
| Lag | Low when idle, rises during heavy work |
| Temperature | Plausible and rises under load |
| **Dynamis Power's own cost** | **"within budget": under 0.5% CPU and 20 MB** |

Attach `power.csv` and the report output to the hardware report issue. The log contains no personal information: only times, power, speed, lag, temperature and brightness.

## Phase 2: the service (after phase 1 passes)

This step **changes settings**, so do it only after the read-only test above looks right. Everything it changes is undone by `sudo dynamis restore`, by uninstalling, or by rebooting before the service is installed.

1. Try one change by hand, with a trial so it undoes itself if the laptop misbehaves:

   ```sh
   sudo dynamis settings
   sudo dynamis set cpu.epp power --trial 120
   ```

   Use the laptop for a minute, then type `y` to keep it or let it time out. Then `sudo dynamis restore`.

2. Install the service: either the packages (see "Installing from packages" in [../daemon/README.md](../daemon/README.md)), or `sudo ./dist/install.sh` from the `daemon` folder.
3. Click the battery icon in the panel and switch between the power modes. Each switch should show up in `sudo dynamis journal`, and `powerprofilesctl` should agree with the panel.
4. Reboot. The mode you chose should still be selected, and `journalctl -u dynamisd -b` should show it being applied.
5. Check `sudo dynamis quirks` and `systemctl list-timers dynamis-sync.timer`: the quirk list check should be scheduled.
6. Uninstall with `sudo apt remove dynamis-power dynamis-power-service` (packages) or `sudo ./dist/uninstall.sh` (script). `powerprofilesctl` should then show power-profiles-daemon's own driver again.

### Auto mode (phase 3)

After installing, log out and back in once (this starts the session agent), then:

1. `dynamis auto on`, then `dynamis auto` every so often while you do different things. It should say what it sees:
   - browsing → light use
   - a video → playing media
   - a video call → on a call
   - locking the screen → nobody is using the laptop (check from another computer over SSH, or just look at `sudo dynamis journal` afterwards)
   - unplugging → on battery, at once
2. Pick a mode in the panel: `dynamis auto` should say it is paused until the charger changes.
3. Anything that looks wrong: note what you were doing and what `dynamis auto` said, and add it to a bug report. It can become a new scenario file.

### Learning (phase 4) and its gate

1. `dynamis learn on`, then use the laptop on battery for light work (or leave it idle) for a few hours over a day or two. `dynamis learn` shows progress; `dynamis learned` shows results.
2. **The gate: does it beat stock Balanced?** On a fully charged battery, with the same brightness and the same light activity (for example a looping video or a fixed set of web pages), run `sudo ./scripts/phase1-test.sh 60` twice: once with `dynamis learn on` and once after `dynamis learn off` (which puts every learned setting back). Compare the average whole-system power in the two reports. Phase 4 passes if the learned settings use measurably less power with no rise in lag.
3. Send both `phase1-results-….tar.gz` files and the output of `dynamis learned --json`.

### Modes on laptops without a speed preference

On the VivoBook (and any laptop where Modes used to show only Balanced):

1. Send the output of `ls /sys/devices/system/cpu/cpufreq/policy0/`, `cat /sys/firmware/acpi/platform_profile_choices` and `cat /sys/devices/system/cpu/intel_pstate/status`.
2. Modes should now offer Longer battery, Balanced and Faster. Pick each and check: `cat /sys/devices/system/cpu/intel_pstate/no_turbo /sys/devices/system/cpu/intel_pstate/max_perf_pct /sys/devices/system/cpu/cpufreq/policy0/scaling_governor` should read `1 60 powersave` for Longer battery, `0 100 powersave` for Balanced and `0 100 performance` for Faster.
3. Compare power on battery between Longer battery and Balanced with the same light use (`sudo dynamis gauge`).

### Sleep test

1. Charge to at least 60%, unplug, save your work, then on the Battery screen choose *Start the sleep test* with 15 minutes (or `dynamis sleep-test on --minutes 15`).
2. The laptop should go to sleep by itself, wake about 15 minutes later, wait a minute, and do the same with the other sleep type. If it does not wake by itself, press the power button and note it: the wake-up alarm may not work on this laptop (`cat /sys/class/rtc/rtc0/wakealarm` while it is set should show a time), or the service's sandbox may block it.
3. Afterwards, check the results (battery used per hour, *Woke cleanly*) and `cat /sys/power/mem_sleep`: the original type should be bracketed again. Wi-Fi, sound, the touchpad and USB devices should all work.
4. **On the VivoBook:** if it freezes while asleep or waking with one type, that is useful: after restarting, the app should record that type as failed. Send `dynamis sleep-test` and `journalctl -b -1 -k | tail -50`.

### Devices (TLP batch 2)

1. On battery, switch on both switches under *Devices* on the Battery screen. Then `grep . /sys/bus/usb/devices/*/power/control /sys/bus/pci/devices/*/power/control`: the devices the screen lists as sleeping should read `auto`; the mouse, keyboard, Bluetooth and graphics should not have changed.
2. Use the laptop for a while: the mouse, keyboard, touchpad, sound, Wi-Fi and a USB stick should all behave as before. Plug in a USB stick: within a reading (15 seconds) it should read `auto` too.
3. Plug the charger in: every device goes back to what it was. *Restore original settings* does the same and switches both off.
4. Compare power on battery with the switches off and on (`sudo dynamis gauge`), with the same light use.

### Learn now

1. Charge to at least 60%, unplug, and in the app press *Learning → Start learning now* (leave the light workload ticked). Leave the laptop alone.
2. The screen should turn off as usual, the laptop should not go to sleep, and the session should run for about two hours. Afterwards open the app: it should say how many changes it kept and the power saved.
3. Start it again and move the mouse once the screen is off: it should stop within a few seconds and say "you used the laptop".
4. Send `dynamis learned` and the journal (`dynamis journal`).

### Screen brightness for each mode

1. In the app, on Modes, switch on *Set the screen brightness for this mode* for Longer battery and move it to 30%. Switch to Longer battery: the screen should dim to about 30% once.
2. Brighten it with the keys. Wait a minute: it should stay where you put it. Switch to Balanced and back: it dims to 30% again.
3. Note whether the desktop's own brightness slider shows the new level. On KDE Plasma it may show the old one until it is moved; say so in the report.

### Tuning together (phase 5) and its gate

1. Leave learning on. Once every setting has been tested on its own, `dynamis learn` shows tests of `cpu.turbo` and `cpu.max_perf_pct` again, one step at a time. `dynamis learned` lists each result with what it was tested together with.
2. **The gate: does it beat a hand-tuned TLP setup?** Install TLP on the same laptop (with Dynamis Power stopped), tune its battery settings by hand for the best power you can get without the laptop feeling slow, and run `sudo ./scripts/phase1-test.sh 60` with the same brightness and the same light activity. Then stop TLP, start Dynamis Power with what it learned, and run it again. Phase 5 passes if Dynamis Power uses no more power, with no rise in lag. Send both result files.

### Prediction (phase 7) and its gate

1. Use the laptop normally for a week or two. The Battery screen's forecast should settle and get closer to how long the battery really lasts; after a few weekdays of unplugging at about the same time, it should show your usual unplug time.
2. If the battery has a charge limit, switch on *Follow my routine*: overnight the limit should read 80 (`cat /sys/class/power_supply/BAT*/charge_control_end_threshold`) and 100 a couple of hours before you usually unplug.
3. **The gate: are time targets met?** From a full charge, pick *Make it last* for a time a little beyond what the forecast says, and use the laptop normally. Note when you started, the target, and when the battery reached 5%. Repeat on a few days with different targets. Phase 7 passes if targets that were reachable are met within about 10 minutes, and Dynamis Power said so when one was not reachable.

### Apps (phase 6) and its gate

1. Open the app's **Apps** screen. After a while on battery, the apps you use should be listed with estimated watt-hours; plugged in, with processor time. The app you are using should be tagged *In front* (KDE Plasma; after logging out and in once so the session agent loads its KWin script).
2. Check the shares really change: switch on *Give the app in front priority*, then `cat /sys/fs/cgroup/user.slice/user-$(id -u).slice/user@$(id -u).service/app.slice/*/cpu.weight` while switching between two apps. The one in front should read 400. *Restore original settings* should put every one back to 100.
3. **The gate: does the app in front stall less under load?** On battery, start a heavy background job in a terminal app (for example `stress-ng --cpu 2 --timeout 120`), then scroll a long page in the browser in front, once with both switches off and once with both on. Compare `cat /proc/pressure/cpu` before and after each run, and how smooth scrolling felt. Phase 6 passes if the app in front waits measurably less with the switches on.

Report anything odd in a bug report with `sudo dynamis journal --last 100` attached.

**On a laptop that already freezes** (like the project's VivoBook), a freeze during these steps does not prove Dynamis Power caused it. After the reboot, Home says what was happening (going to sleep, waking, a device error, or nothing in the log) and what Dynamis Power had changed in the ten minutes before; `sudo dynamis journal` shows the same changes, and a `recover` line means a change was in progress. For the system's own log of that boot to be there, the journal must be kept on disk (`/var/log/journal` exists). After three such freezes safe mode switches on: keep using the laptop. If it still freezes, `dynamis safe-mode` says Dynamis Power is unlikely to be the cause; report that with `journalctl -b -1 -k | tail -50`.

## Known limits

- Whole-system power is only meaningful on battery. Plugged in, most laptops report nothing useful.
- The CPU energy counters usually need `sudo`.
- Some laptops update the battery reading only every 30–60 seconds. That's normal, and the gauge check will say so.
