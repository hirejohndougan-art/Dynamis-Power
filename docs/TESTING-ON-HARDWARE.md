# Testing on real hardware

This is the phase 1 test: it checks that Dynamis Power reads a real laptop correctly and costs almost nothing to run. **Every step only reads.** Nothing on the laptop is changed.

It takes about 1½ hours, most of it the laptop running normally on battery.

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
sudo dynamis log -o power.csv
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

2. Install the service: `sudo ./dist/install.sh` from the `daemon` folder.
3. Click the battery icon in the panel and switch between the power modes. Each switch should show up in `sudo dynamis journal`, and `powerprofilesctl` should agree with the panel.
4. Reboot. The mode you chose should still be selected, and `journalctl -u dynamisd -b` should show it being applied.
5. Uninstall with `sudo ./dist/uninstall.sh`. `powerprofilesctl` should then show power-profiles-daemon's own driver again.

Report anything odd in a bug report with `sudo dynamis journal --last 100` attached.

**On a laptop that already freezes** (like the project's VivoBook), a freeze during these steps does not prove Dynamis Power caused it. After the reboot, check `sudo dynamis journal`: a `recover` line means a change was in progress at the time; no such line means nothing was being changed.

## Known limits

- Whole-system power is only meaningful on battery. Plugged in, most laptops report nothing useful.
- The CPU energy counters usually need `sudo`.
- Some laptops update the battery reading only every 30–60 seconds. That's normal, and the gauge check will say so.
