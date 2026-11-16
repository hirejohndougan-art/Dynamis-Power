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

2. Install the service: `sudo ./dist/install.sh` from the `daemon` folder.
3. Click the battery icon in the panel and switch between the power modes. Each switch should show up in `sudo dynamis journal`, and `powerprofilesctl` should agree with the panel.
4. Reboot. The mode you chose should still be selected, and `journalctl -u dynamisd -b` should show it being applied.
5. Check `sudo dynamis quirks` and `systemctl list-timers dynamis-sync.timer`: the quirk list check should be scheduled.
6. Uninstall with `sudo ./dist/uninstall.sh`. `powerprofilesctl` should then show power-profiles-daemon's own driver again.

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

Report anything odd in a bug report with `sudo dynamis journal --last 100` attached.

**On a laptop that already freezes** (like the project's VivoBook), a freeze during these steps does not prove Dynamis Power caused it. After the reboot, check `sudo dynamis journal`: a `recover` line means a change was in progress at the time; no such line means nothing was being changed.

## Known limits

- Whole-system power is only meaningful on battery. Plugged in, most laptops report nothing useful.
- The CPU energy counters usually need `sudo`.
- Some laptops update the battery reading only every 30–60 seconds. That's normal, and the gauge check will say so.
