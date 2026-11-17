# User interface

The app must be usable by someone who has never opened a terminal, while still giving experienced users full control. It does this with layers: simple on top, detail one click down, and everything in **Advanced**.

The visual mock-ups are kept with the project's design files. This document is the written specification; when the two disagree, update both.

> **Built so far** (see [../gui/README.md](../gui/README.md)): the first-run welcome, Home, Modes (with the panel icon switch), Apps, Battery (with the forecast, *Make it last* and *Follow my routine*), Activity (with Logs and Export), and every tab of Advanced. The mode control has three steps, one per standard profile. Everything else on this page is the plan.

## Principles

1. **Plain words first.** "Saving battery while you browse", not "EPP balance_power". Technical names appear only in Advanced and in the technical log.
2. **Every automatic change is visible and reversible.** Each one shows a reason, a measured result and Undo.
3. **One obvious control.** The slider from *Longer battery* to *Faster* is always on the home screen.
4. **Nothing risky without a clear yes.** Risky settings are off until switched on individually, with a plain explanation.
5. **Quiet.** Few notifications, none during calls or full-screen video.
6. **Accessible.** Full keyboard use, screen-reader labels, readable contrast, scalable text, follows the desktop's light or dark theme.

**Colours.** Canary yellow (`#ffef00`) is the app's colour: buttons, switches, bars, the selected mode and the chart's fill. On light backgrounds yellow is too faint for text and thin lines, so those use a dark gold (`#6b5a00`, the `--accent-strong` token), and text on yellow is near-black. In dark mode yellow works for both. The app icon and the panel icon for Balanced are yellow with a dark bolt (`gui/scripts/make-icons.py` draws the app icons).
7. **Free when closed.** The window costs nothing when it isn't open. The panel icon is drawn by the session agent that runs anyway, so it adds no process; it can be hidden.

## Navigation

A sidebar, in this order: Dashboard, Modes, Apps, Battery, Devices, Learning, Activity, then Advanced, always last and visually separated. On a computer without a battery, Battery is left out ([decision 0023](decisions/0023-desktops-two-gpus-image-based-flatpak.md)); Modes then asks only for the mode for ordinary use, and Learning explains that it cannot measure power there yet.

| Section | Who it's for | Purpose |
| --- | --- | --- |
| Dashboard | Everyone | How things stand and the one main control |
| Modes | Everyone | Choose or schedule profiles; "Last until" |
| Battery | Everyone | Health, charge limit, forecast, charging |
| Devices | Everyone | Let idle USB devices and built-in hardware sleep on battery; every device listed by its friendly name (from the system's `pci.ids` and `usb.ids`, as `lspci` and `lsusb` use), with why some stay awake and *Keep awake* per device |
| Learning | Everyone | How far learning has got (a percentage, tests and quiet time left), a note while learned settings are re-checked after a system update or a few months on (built), what it kept with Forget, the tests so far, *Learn now* and the sleep test |
| Apps | Everyone | What uses power; per-app rules. Dynamis Power's own app and panel icon are never listed |
| Activity | Everyone | Plain-language history of changes, with Undo; logs and export |
| Sharing | Everyone | Opt-in results sharing, what's sent, delete my data |
| Advanced | Experienced users | Every setting, learning controls, sensors, profiles, safety, developer tools |

## Dashboard

The first screen (called Home in older notes). 
- **Power use, at the top:** watts now, the average, the hottest temperature, and a chart of the last 30 minutes with grid lines (watts from zero up the side, minutes along the bottom) and a dashed temperature line with its own scale on the right (built). Readings come from the app while it is open on battery; plugged in, the chart says most laptops report nothing while charging.
- **Battery meter:** a battery-shaped bar across the time-left card, filled to the charge with the percentage inside ("79%", "79% · charging"); orange at 20% or less.
- **Big estimate:** "About 4 h 10 min left" as a range ("3 h 50 – 4 h 30"), or "Fully charged in 40 min".
- **Status sentence:** "Saving battery while you browse." with the active mode as a chip.
- **The slider:** Longer battery ↔ Faster, five steps.
- **Something stopped working** (built), next to *Restore original settings* (and in the panel menu): one click undoes the last hour, then a dialog lists what was undone and asks which of these stopped working (Wi-Fi, Bluetooth, Sound, Mouse, keyboard or touchpad, The screen, A disk or USB drive, Sleep or waking up, The laptop froze, Something else), with *It works again*, *Later* and *Done*. The result says what was put back and blocked, or that nothing Dynamis Power changed can affect it. After *Later*, a notice keeps the question open. See [EVERYDAY-ISSUES.md](EVERYDAY-ISSUES.md) §1.
- **It feels slow** (built), under the mode: a confirmation with *Keep it this way*, then "Faster settings until midnight" with what was undone and *End now*. See [EVERYDAY-ISSUES.md](EVERYDAY-ISSUES.md) §3.
- **Offers from habits** (built): when the same mode was picked by hand three times with the same app in front, or around the same hour on three days, a notice offers to do it automatically ("You picked Longer battery around 9 PM on 3 different days. Use Longer battery by itself from 9 PM to 11 PM?") with *Yes, do that* and *No thanks*. Nothing changes without a yes; a no is not asked again.
- **Runs hotter** (built): "At light use, this laptop now runs about 7 °C warmer than in March 2026…" with the usual cause (dust in the vents), only when it has risen 5 °C or more over three months.
- **Learning card:** "Learning your laptop — 6 of 18 settings checked. Saved about 25 min so far." with a link to Activity.
- **Last change card:** the most recent automatic change, its result and Undo.
- **Safety strip** (only when relevant): "A test was undone because Wi-Fi dropped. Nothing else changed."

## Modes

Cards for each profile with a one-line description. "Last until" opens a time picker and shows whether the target looks achievable. Rules ("When I open Steam, use Gaming") are added with a simple *When … use …* builder. **By time of day** (built, in the automatic choice card): a list of "9 PM to 11 PM: Longer battery" with *Remove*, and *From*, *To*, *Mode* and *Add*; saved with the other automatic choices. Battery and app rules come first.

**Screen brightness for each mode (built).** Each mode card has *Set the screen brightness for this mode* and, when on, a slider (5–100% in steps of 5). Under it, from the learned screen model: what the screen uses at that level and how many minutes more or less the battery lasts than at the brightness now ("No estimate yet" until the model has enough). When a mode starts, by hand, by auto mode, by an app or by *Make it last*, the screen is set to its level once; between mode changes the person's brightness keys win, so it never fights them. Choosing a level for the mode in use sets it at once. *Suggest levels* (shown while no mode has a level) starts Longer battery at no more than 40% and Balanced at the brightness now, and leaves Faster alone. Saved half a second after the slider stops. Restore forgets the levels and leaves the screen as it is.

## Battery

Health percentage against design capacity, wear trend, charge limit toggle (only where supported), adaptive charging, overnight drain report, and charge history. **Built:** the condition (health, capacity, cycles), the forecast, *Make it last* and *Follow my routine* as four cards, two by two (one column on a narrow window); the charge limit below them; and at the bottom *What uses the battery*: the screen (from the learned screen model at the brightness now), the processor and graphics (the chip's energy counters, graphics taken out of the processor's share where the chip reports it), and everything else (what is left of the battery's own measurement, on battery only), largest first, in watts and as a share, measured every 3 seconds while the page is open, with a link to the brightness for each mode. *Recalibrate the battery* (only where the battery can run the laptop while plugged in; [decision 0019](decisions/0019-running-from-battery-while-plugged-in.md)): a plain explanation, a confirmation, the phase with the percentage and *Stop*, and afterwards what the gauge says the battery holds before and after. Also built: the forecast at your usual use and usual unplug time, *Make it last* (2/4/6/8 hours or a clock time; shows the budget and recent use; *Stop*), *Follow my routine* (charging that follows habits), and, until they moved, the sleep test (now under Learning) and the device switches (now their own Devices screen). *While asleep* (built): the last sleeps with the date, how long, the battery used per hour, and what woke the laptop in plain words ("Woken by a USB device"); above 2.5% an hour it adds what may help. Still planned: wear trend, charge history. "Last until" lives on Battery rather than Modes, next to the forecast it depends on; Home shows it while it runs.

## Apps

A ranked list of apps by estimated energy today and this week, clearly marked as estimates. Plugged in, where there are no battery watts to share out, the list ranks by processor time instead. Each app has an *Options* dialog: share of the processor (*Normal*, *Never slow it down*, *Always slow it down*) and *Mode while it is open*. Below the list: *Slow down apps in the background on battery*, *Give the app in front priority*, *Count battery use per app*, and *Forget app history*. Tags on each row say whether it is open or in front, its mode, and whether it is slowed or has priority right now. Under the list, an app whose average power while running on battery changed by a quarter or more since last month gets one line ("Firefox uses 43% more power than last month while running on battery (1.4 W → 2.0 W). An update or an add-on is the usual cause."). **(Built.)** Still planned: a warning with one action for misbehaving apps.

## Learning

**Learn now** (built): start a baseline session, leave the laptop unplugged and alone for about two hours, and every test runs back to back, with a light workload for the settings whose cost is lag (a checkbox in the confirmation; without it those settings are left for everyday learning). It waits for the screen to turn off before the first test (and says so, or that the session helper is not running so it cannot tell). Shows what it needs, the test running and time left, and each result as Kept, Not kept or Unclear; any use of the laptop, plugging in, a low battery, warmth or a call stops it and says why, keeping the tests already finished. Learning stays on afterwards.

**Built.** Its own place in the main menu. The top card shows the switch and progress as a percentage of the tests this laptop needs, with a bar, the stage (one setting at a time, or processor settings together), and about how many tests and how much quiet time on battery are left. The percentage is an estimate: while settings are tuned together, a kept change can open a step or two more, so it can step back slightly; it reads 100% only when nothing is left to test. Below: what it kept (plain names, what each was, what it saves, *Forget*) and the tests so far. The technical table stays in Advanced → Learning. Home's learning card shows the same percentage and bar.

## Activity, logs and export

**Activity** is the plain-language history. Each entry: time, what changed, why, measured result, Undo. Filters: *Automatic changes*, *My changes*, *Tests*, *Rollbacks*, *Problems*.

**Logs** tab (inside Activity) shows the technical log: the full change journal with setting IDs, old and new values, trial status and errors, plus daemon messages. It has search, level filter (info, warning, error), time range, and a live-follow toggle.

**Export logs** opens a dialog:

| Option | Choices |
| --- | --- |
| What to include | Change journal · Daemon log · Experiment results · Hardware summary · Current settings · Sensor samples |
| Time range | Last 24 hours · Last 7 days · Last 30 days · All · Custom |
| Format | Bundle for a bug report (`.zip`) · JSON · CSV · Plain text |
| Privacy | *Remove personal details* (on by default): strips usernames, hostnames, serial numbers, MAC addresses, network names and app names |

A **preview** shows exactly what will be exported before saving. **Copy summary** puts a short text summary on the clipboard for pasting into a GitHub issue. Export never sends anything anywhere; it only saves a file.

## Sharing

Off by default. Shows, field by field, what would be shared, then a single Share button. Also: share automatically after each new result (off by default), *Delete my submissions*, and a link to the privacy policy.

## Advanced

Advanced opens with a one-line notice: "These controls are for experienced users. Everything here can be undone."

Advanced always shows the technical details (setting IDs, raw kernel values, allowed ranges, why a value is not allowed here). There is no Details toggle here; the plain-language rule with a Details toggle applies to the main screens.

| Tab | Contents |
| --- | --- |
| Settings | Every setting the app manages, grouped by component. Each row: plain name, technical ID, current value, learned best value, risk tier badge, status (*learned*, *testing*, *pinned*, *blocked*), and controls to pin a value, exclude it from learning, or reset it. Risky and experimental settings each need their own switch |
| Learning | Pause learning; how much time to spend testing (low, normal, eager); test only on battery / also plugged in; allow running from battery while plugged in; reset everything learned (with confirmation) |
| Profiles | **Built:** choose what each mode sets on this laptop (from the values it offers), back to standard, export and import as a file. *Planned:* goals, limits and cloning, with extended profiles |
| Sensors | **Built:** live readings every 2 s while open: whole-laptop and processor power, time in the deepest sleep state, average speed (with small charts), waiting (stall) for processor, memory and disk, temperatures, brightness |
| Safety | Safe-mode status and switch, whether the kernel log is watched for trouble after changes (built), *Report a problem* (built), quirk list version and blocked settings for this machine, trial in progress, rollback history, dead-man timer length, battery floor for tests |
| System | **Built:** versions, the service's own memory and processor use against its budget, *Import from TLP* (the TLP configuration as Dynamis Power would use it, line by line with a checkbox each, the lines not imported with why, then *Import*; [decision 0017](decisions/0017-importing-a-tlp-configuration.md)), other power tools with *Switch off*, what the desktop controls (sleep is never touched; brightness only when a brightness is chosen for a mode), where updates come from |
| Developer | **Built:** dry run (changes journaled as "would", nothing written; learning waits; restore still writes), advisor (the mode in use plus what was learned, as a TLP or TuneD configuration, shown and saved as a file, never installed), raw data from the service, *Restore original settings*. *Not built:* log level (the service logs to the journal; `journalctl -u dynamisd`) |

## Panel icon

**Built.** An icon next to the clock (StatusNotifierItem with a DBusMenu menu, drawn by the session agent `dynamis-agent`), shown from login on every desktop that supports panel icons (KDE Plasma, Xfce, Cinnamon, MATE, Budgie; GNOME with the AppIndicator extension, which Ubuntu ships on).

- The icon is the app's logo, coloured by mode (green Longer battery, canary yellow Balanced with a dark bolt, orange Faster, grey when the service is not running or safe mode is on). The tooltip says the mode, battery level and time left.
- **Left click** opens the app.
- **Menu:** a summary line; *Longer battery*, *Balanced*, *Faster* (radio); *Choose automatically* (check); *Make it last* ▸ 2, 4 or 6 hours, or *Stop making it last*; *It feels slow*; *Something stopped working…* (undoes the last hour and opens the app to ask what stopped working; works in safe mode too); *Report a problem…* (opens the app with that dialog); *Open Dynamis Power*; *Hide this icon*.
- Every change goes to the service exactly as the app sends it (polkit, safe mode and every rail apply). In safe mode the controls are greyed out.
- *Hide this icon*, or the switch on Modes, turns it off; the choice is remembered per person.

## Dialogs and notifications

- **Risky setting switch:** explains what can go wrong in one sentence and requires a deliberate confirmation.
- **Rollback notice:** one notification, at most, saying what was undone and why.
- **Restore original settings:** available from Home and Advanced, and always works, even in safe mode. The panel icon does not offer it, because it asks for confirmation; *Open Dynamis Power* is one click away.
- **First run:** a four-step welcome — what the app does, hardware check, conflicts found (with a fix button), and a baseline measurement. Nothing changes until the user clicks *Start*.

## Strings and translation

Every user-facing string goes through the translation layer. Technical IDs are never translated. Sentence case for all labels and buttons.
