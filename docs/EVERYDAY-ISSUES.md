# Problems people may hit, and the plan for each

What can go wrong for someone using Dynamis Power day to day, what already protects them, and the plan for the rest. Each plan has a to-do in [../ROADMAP.md](../ROADMAP.md) ("To do: everyday problems"). Anything that needs research, real hardware or a decision first is listed under **Follow-up** at the end and in the roadmap's follow-up list.

## 1. Hardware that misbehaves when power saving is on

**What happens.** Wi-Fi drops, a USB mouse lags or stops after autosuspend, sound pops when it starts, the screen flickers, or the laptop freezes when PCIe link power saving is forced (the Realtek RTL8821CE case).

**Already in place.** One change at a time; trials undo themselves unless the laptop stays healthy; the health monitor watches the kernel log during tests; the signed blocklist of known-bad settings per model; safe mode after three unclean shutdowns; Restore.

**Plan.**

- **"Something stopped working" in the app and the panel menu.** One click undoes every change made in the last hour (newest first, each journaled), shows what was undone, and asks which device misbehaved (Wi-Fi, sound, mouse or keyboard, screen, other). The answer blocks the matching settings on this laptop and goes into the report described in section 10, so the problem can reach the blocklist for everyone with that model.
- **Watch the kernel log all the time, not only during tests.** The health monitor's patterns (device resets, firmware crashes, GPU hangs, Wi-Fi disconnects with driver errors) are checked on each reading. A match within ten minutes of a change undoes that change and blocks it; a match with no recent change is only logged, so unrelated faults do not block good settings.
- **Device settings start conservative.** USB autosuspend never touches input, audio, printers, phones, mobile broadband or Bluetooth adapters; Wi-Fi power saving checks the blocklist and a built-in list of chips with known trouble before it is offered.

## 2. Learning that seems stuck

**What happens.** Learning only tests on battery when the laptop is quiet. A laptop that is always plugged in never learns; a busy one waits ("heavy work right now"); a battery that reports power coarsely gives "unclear, will retry" over and over.

**Plan.**

- **Say exactly why, with what to do.** Learning keeps a count of why it waited over the last day and shows the main reason with advice: "Plugged in all day: learning needs time on battery; it will continue next time you unplug", "Busy most of the time on battery: it tests during quiet moments, such as when you are reading", "This laptop reports power only once a minute: each test takes longer here".
- **Measure the battery's own update rate.** On the first readings on battery, time how often the power value actually changes. Test blocks become at least four update periods long, so slow-reporting batteries still give clear results, and the time-left estimate uses the real block length.
- **Use energy when power is missing.** Where a battery reports energy left but no power, compute power from the change in energy over time (coarser, so blocks are longer). Where it reports neither, say that learning cannot measure on this laptop instead of waiting forever.
- **Learning while plugged in, where safe.** On laptops that can run from the battery while plugged in (`battery.charge_behaviour`), offer to do tests that way, within strict limits: battery between 40% and 80%, cool, at most one hour a day. Off by default; see Follow-up.

## 3. "It feels slower"

**What happens.** Longer battery, a learned speed limit, or a background app being slowed makes the laptop feel sluggish in ways the lag measure did not catch. Auto mode may switch at moments people do not expect.

**Plan.**

- **"It feels slow" button** on Home and in the panel menu. It shows what is in effect right now in plain words (mode, learned changes, apps slowed), undoes the speed-related ones for the rest of the day, and offers to make the choice lasting: learning then keeps only changes with no measurable lag at all (limit 0 instead of 0.5 points), and auto mode prefers Balanced over Longer battery for ordinary use.
- **Say why auto mode switched, where people look.** The panel icon's tooltip already shows the mode; it gains the last reason ("Longer battery: nobody is using the laptop"). Activity groups auto-mode switches into one line per hour so they do not flood it.
- **Notice repeated undo.** If the same learned change is undone by hand twice, it is blocked on this laptop (the person has spoken), with a note in Learning.

## 4. Other software changing the same settings

**What happens.** A second tool changes a setting back: `powertop --auto-tune` at boot, vendor tools (asusctl, System76 Power, Slimbook, TUXEDO Control Center), laptop-mode-tools, distribution scripts, udev rules. Each undoes the other and nobody can tell why. TLP, power-profiles-daemon and tuned-ppd are already handled.

**Plan.**

- **Notice changes made behind Dynamis Power's back.** On each reading, compare the settings Dynamis Power last set (from the journal) with what the kernel reports. A difference it did not make is journaled as "changed outside Dynamis Power".
- **Name the likely tool.** Check for known tools: running services and processes, `/etc/udev/rules.d` and `/usr/lib/udev/rules.d` rules that write the same files, `powertop` services, systemd units. The app says "Asus Linux (asusctl) changed the battery charge limit back to 100%".
- **Never fight.** After two outside changes to the same setting in a day, Dynamis Power stops managing that setting (keeping its own journal honest) and asks the person which tool should own it. The known-tools list in the welcome and System grows to include these tools, with Switch off where it is safe to do so.

## 5. Desktop differences

**What happens.** The panel icon needs the AppIndicator extension on plain GNOME; knowing which app is in front only works on KDE Plasma; the desktop's own settings still dim the screen and put the laptop to sleep, and people may think Dynamis Power did it.

**Plan.**

- **Check and explain at first run.** The welcome and Modes detect GNOME without AppIndicator (no StatusNotifierWatcher on the session bus) and say how to turn it on, with the extension's name.
- **A small GNOME Shell extension** shipped in the app package (not on by default) that tells the session agent which window is in front, as the KWin script does. See Follow-up for distribution.
- **Say who did what.** When the screen dims or the laptop sleeps, that is the desktop; Activity only lists Dynamis Power's own changes, and Advanced → System already says that brightness and sleep belong to the desktop. Battery gains a line linking to the desktop's power settings.

## 6. Kernel and firmware updates

**What happens.** An update adds, removes or changes controls (new drivers, new AMD modes, a renamed file). Learned results may no longer be true; a setting may vanish.

**Plan.**

- **Record what each result was measured on.** Every test result records the kernel version and the firmware (BIOS) version and date. At startup the service compares them with the running ones.
- **Re-check after a change.** Learned settings stay applied (they were fine before), but each is re-tested first, ahead of new tests, in the next quiet moments; a re-test that no longer saves power, or adds lag, puts it back. Learning shows "Re-checking after a system update".
- **Settings that vanish or change are handled, not errors.** A learned setting whose control no longer exists is dropped from what is applied, with a line in Activity; a changed list of choices re-plans the modes.
- **Quirks by version.** The signed blocklist gains optional kernel and firmware version ranges, so a fix in a newer kernel lifts a block automatically.

## 7. Battery and charging quirks

**What happens.** Every Dynamis Power change is runtime-only, so a charge limit set by hand is gone after a reboot; some laptops also reset it after sleep; some firmware ignores it; people forget the battery is held at 80% before a trip.

**Plan.**

- **Settings a person chose by hand are kept.** A small list (in the service's own state, not system configuration) of settings set by hand in the app (charge limit, charge start, and any Advanced setting with "Keep after restart" ticked). The service re-applies them at startup and after waking from sleep, through the engine as usual. They are not re-applied in safe mode, nor after an unclean shutdown that happened within ten minutes of one of them being set (so a bad setting cannot come back on its own). This keeps rule 3 (no persistent system configuration) while giving people what they expect.
- **Check that the firmware listened.** The engine already reads every value back. For the charge limit, also check later: if the battery keeps charging past the limit (a percentage reading above the limit plus two while plugged in), say that this laptop's firmware ignores it, and stop offering it here.
- **"Charge fully once".** A button on Battery and in the panel menu that lifts the limit until the battery is full or unplugged, then puts it back (already on the TLP list).

## 8. Safe mode on laptops that freeze anyway

**What happens.** Three unclean shutdowns in a row switch safe mode on, even when Dynamis Power had nothing to do with them (the VivoBook freezes on its own). It can look as if the app broke.

**Plan.**

- **Say what was happening when it stopped.** After an unclean shutdown, read the previous boot's kernel log (`journalctl -b -1`, read-only): going to sleep, waking, a driver error, or nothing. Combine with the journal: was a change in progress, or made in the last ten minutes? Show this on Home in plain words.
- **Use safe mode as a test.** With safe mode on, Dynamis Power changes nothing. If freezes continue in safe mode, the cause is elsewhere: the app says so ("The laptop still froze twice with Dynamis Power changing nothing, so it is unlikely to be the cause") and offers to switch safe mode off. If freezes stop, the most recent changes before safe mode are the suspects, and they stay blocked when it is switched off.
- **Count only what could be ours.** See Follow-up: whether a freeze long after the last change should count towards safe mode at all.

## 9. Install and platform limits

**What happens.** Only Debian and Ubuntu packages exist. Systemd and polkit are required. Changes are refused over SSH (by design). Desktops without a battery, two-GPU laptops and image-based systems (Fedora Silverblue, SteamOS) are not supported.

**Plan.**

- **Packages for Fedora (RPM) and Arch (PKGBUILD, for the AUR)**, built by CI next to the `.deb`, with the same install and clean-removal steps (enable the service, switch off and remember replaced tools, restore everything on removal).
- **Refuse clearly where it cannot work.** At startup the service checks for systemd and polkit, and the app explains what is missing, instead of failing silently.
- **Desktops**: auto mode and app priority make sense without a battery (heat, noise, electricity); learning needs a power reading, which desktops rarely have. See Follow-up.

## 10. Getting help

**What happens.** When something goes wrong, a person who has never used a terminal needs to hand someone everything useful in one step.

**Plan.**

- **"Report a problem"** in Activity, Advanced and the panel menu: builds the existing export (with the privacy filter on), adds the answers from section 1's "Something stopped working", the diagnosis from section 8 and the outside changes from section 4, saves it as one file, and opens a pre-filled issue page on GitHub with a plain summary and no personal data, asking the person to attach the file.
- **A diagnostic report like `tlp-stat`** (already on the TLP list): `dynamis report --full`, also included in the export.

## Follow-up: needs research, hardware or a decision

- **Learning on busy, slow laptops.** Whether tests can run during steady moderate use (judging quiet by how stable power is rather than by how busy the processor is), and what the "heavy work" threshold should be on 2-core processors. Needs readings from real laptops, the VivoBook first.
- **Battery power update rates.** How often real batteries refresh power and energy, and how noisy they are, across makers; this sets the minimum test block length. Collect from the phase 1 logs.
- **Testing while plugged in** (`charge_behaviour` = inhibit/force-discharge): which laptops support it, whether firmware behaves (some keep charging, some heat), and what limits keep the battery safe. Decision record needed before building.
- **Vendor tools.** For each (asusctl, supergfxctl, System76 Power, Slimbook, TUXEDO Control Center, laptop-mode-tools, `powertop --auto-tune`): does it conflict, can both run, and should Dynamis Power offer to switch it off. thermald is expected to be compatible (it manages heat, not power modes); confirm.
- **GNOME extension.** Whether to publish on extensions.gnome.org (review process, keeping up with GNOME versions) or ship only in the package; and whether GNOME's own APIs can report the focused window without an extension.
- **Which laptops reset the charge limit** after reboot or sleep, and which ignore it; collect per model (shared results would help).
- **Safe-mode counting.** Whether a freeze hours after the last change should count towards safe mode. Counting every freeze is the safe default; counting fewer avoids blaming the app wrongly. Decide after real freeze data from the VivoBook.
- **"Feels slow" and lag.** Whether the stall measure catches what people notice (input latency, frame drops), and whether a stricter or additional measure is needed. Needs people's reports against measurements.
- **Desktops, two-GPU laptops, image-based systems, Flatpak.** A Flatpak cannot install the system service; whether to ship the app as a Flatpak that needs the service from a package. Each needs its own decision record.
- **Wi-Fi power saving and the service's lack of network access** (already noted for the TLP list): a separate helper or a narrow, documented exception.
