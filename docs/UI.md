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
7. **Free when closed.** The window costs nothing when it isn't open. The panel icon is drawn by the session agent that runs anyway, so it adds no process; it can be hidden.

## Navigation

A sidebar with six main sections. Advanced is always last and visually separated.

| Section | Who it's for | Purpose |
| --- | --- | --- |
| Home | Everyone | How things stand and the one main control |
| Modes | Everyone | Choose or schedule profiles; "Last until" |
| Battery | Everyone | Health, charge limit, history |
| Learning | Everyone | How far learning has got (a percentage, tests and quiet time left), what it kept with Forget, the tests so far |
| Apps | Everyone | What uses power; per-app rules |
| Activity | Everyone | Plain-language history of changes, with Undo; logs and export |
| Sharing | Everyone | Opt-in results sharing, what's sent, delete my data |
| Advanced | Experienced users | Every setting, learning controls, sensors, profiles, safety, developer tools |

## Home

- **Big estimate:** "About 4 h 10 min left" as a range ("3 h 50 – 4 h 30"), or "Fully charged in 40 min".
- **Status sentence:** "Saving battery while you browse." with the active mode as a chip.
- **The slider:** Longer battery ↔ Faster, five steps.
- **Live draw:** current watts with a small 30-minute sparkline.
- **Learning card:** "Learning your laptop — 6 of 18 settings checked. Saved about 25 min so far." with a link to Activity.
- **Last change card:** the most recent automatic change, its result and Undo.
- **Safety strip** (only when relevant): "A test was undone because Wi-Fi dropped. Nothing else changed."

## Modes

Cards for each profile with a one-line description. "Last until" opens a time picker and shows whether the target looks achievable. Rules ("When I open Steam, use Gaming") are added with a simple *When … use …* builder.

## Battery

Health percentage against design capacity, wear trend, charge limit toggle (only where supported), adaptive charging, overnight drain report, and charge history. **Built:** health, capacity, cycles, charge limit, what the screen costs, the forecast at your usual use and usual unplug time, *Make it last* (2/4/6/8 hours or a clock time; shows the budget and recent use; *Stop*), *Follow my routine* (charging that follows habits), and *Devices* (two switches to let idle USB devices and built-in hardware sleep on battery, with each device listed under a Details toggle and why some are kept awake). Still planned: wear trend, overnight drain report, charge history. "Last until" lives on Battery rather than Modes, next to the forecast it depends on; Home shows it while it runs.

## Apps

A ranked list of apps by estimated energy today and this week, clearly marked as estimates. Plugged in, where there are no battery watts to share out, the list ranks by processor time instead. Each app has an *Options* dialog: share of the processor (*Normal*, *Never slow it down*, *Always slow it down*) and *Mode while it is open*. Below the list: *Slow down apps in the background on battery*, *Give the app in front priority*, *Count battery use per app*, and *Forget app history*. Tags on each row say whether it is open or in front, its mode, and whether it is slowed or has priority right now. **(Built.)** Still planned: a warning with one action for misbehaving apps.

## Learning

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

| Tab | Contents |
| --- | --- |
| Settings | Every setting the app manages, grouped by component. Each row: plain name, technical ID, current value, learned best value, risk tier badge, status (*learned*, *testing*, *pinned*, *blocked*), and controls to pin a value, exclude it from learning, or reset it. Risky and experimental settings each need their own switch |
| Learning | Pause learning; how much time to spend testing (low, normal, eager); test only on battery / also plugged in; allow running from battery while plugged in; reset everything learned (with confirmation) |
| Profiles | **Built:** choose what each mode sets on this laptop (from the values it offers), back to standard, export and import as a file. *Planned:* goals, limits and cloning, with extended profiles |
| Sensors | **Built:** live readings every 2 s while open: whole-laptop and processor power, time in the deepest sleep state, average speed (with small charts), waiting (stall) for processor, memory and disk, temperatures, brightness |
| Safety | Safe-mode status and switch, quirk list version and blocked settings for this machine, trial in progress, rollback history, dead-man timer length, battery floor for tests |
| System | **Built:** versions, the service's own memory and processor use against its budget, other power tools with *Switch off*, what the desktop controls (brightness and sleep are never touched), where updates come from |
| Developer | **Built:** dry run (changes journaled as "would", nothing written; learning waits; restore still writes), advisor (the mode in use plus what was learned, as a TLP or TuneD configuration, shown and saved as a file, never installed), raw data from the service, *Restore original settings*. *Not built:* log level (the service logs to the journal; `journalctl -u dynamisd`) |

## Panel icon

**Built.** An icon next to the clock (StatusNotifierItem with a DBusMenu menu, drawn by the session agent `dynamis-agent`), shown from login on every desktop that supports panel icons (KDE Plasma, Xfce, Cinnamon, MATE, Budgie; GNOME with the AppIndicator extension, which Ubuntu ships on).

- The icon is the app's logo, coloured by mode (green Longer battery, teal Balanced, orange Faster, grey when the service is not running or safe mode is on). The tooltip says the mode, battery level and time left.
- **Left click** opens the app.
- **Menu:** a summary line; *Longer battery*, *Balanced*, *Faster* (radio); *Choose automatically* (check); *Make it last* ▸ 2, 4 or 6 hours, or *Stop making it last*; *Open Dynamis Power*; *Hide this icon*.
- Every change goes to the service exactly as the app sends it (polkit, safe mode and every rail apply). In safe mode the controls are greyed out.
- *Hide this icon*, or the switch on Modes, turns it off; the choice is remembered per person.

## Dialogs and notifications

- **Risky setting switch:** explains what can go wrong in one sentence and requires a deliberate confirmation.
- **Rollback notice:** one notification, at most, saying what was undone and why.
- **Restore original settings:** available from Home and Advanced, and always works, even in safe mode. The panel icon does not offer it, because it asks for confirmation; *Open Dynamis Power* is one click away.
- **First run:** a four-step welcome — what the app does, hardware check, conflicts found (with a fix button), and a baseline measurement. Nothing changes until the user clicks *Start*.

## Strings and translation

Every user-facing string goes through the translation layer. Technical IDs are never translated. Sentence case for all labels and buttons.
