# Dynamis Power

**Battery and performance tuning for Linux laptops that learns your machine.**

> **Status: early planning. Nothing to install yet.** Follow the [roadmap](ROADMAP.md) to see where things stand.

Most Linux power tools apply the same fixed rules to every laptop. Dynamis Power measures *your* laptop instead. It starts from safe, standard settings, then tests small changes one at a time and keeps only the ones that measurably help, whether that means longer battery life, a faster system, or a cooler, quieter one.

## What it will do

- **One simple slider** from "Longer battery" to "Faster", plus ready-made modes for video, calls, gaming and long days away from a charger.
- **"Last until" mode.** Tell it "5 hours" or "until 6 pm" and it budgets power to get you there.
- **Learns on battery and plugged in.** On battery it saves watts. Plugged in, it learns the fastest settings your laptop can hold without overheating.
- **Explains everything.** Every automatic change comes with a plain-language reason, a measured result, and an Undo button.
- **Per-app control.** The app you're using gets priority; background apps are calmed down on battery.
- **Shared results.** Optionally share what worked on your laptop, so the next person with the same model starts from proven settings.

## Built to be safe

Dynamis Power changes system settings, so safety comes first:

- Only settings your hardware offers, within the limits the driver sets.
- One change at a time, with automatic rollback if anything goes wrong.
- Nothing survives a reboot until it has proven itself over days of normal use.
- A safe mode that restores stock settings if your laptop ever fails to boot cleanly.
- It never touches voltages, firmware, BIOS, or thermal protection.
- Uninstalling returns every setting to how it was.

See [docs/SAFETY.md](docs/SAFETY.md) for the full list.

## How it fits with other tools

Dynamis Power replaces power-profiles-daemon, TLP, auto-cpufreq or TuneD rather than running beside them, since those tools change the same settings. It speaks the same interface the KDE and GNOME power sliders already use, so your desktop's controls keep working.

## Planned stack

- **Daemon:** Rust, running as a small system service.
- **App:** Tauri with React, light enough for 4 GB laptops.
- **Shared results:** an optional, anonymous submission service with public per-model summaries.

## Get involved

The project is just starting. The most useful help right now:

- **Hardware reports.** Tell us what laptop you have and what your system exposes. Use the [hardware report form](../../issues/new?template=hardware_report.yml).
- **Ideas and feedback.** Open a [feature request](../../issues/new?template=feature_request.yml).
- **Code.** See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

[MIT](LICENSE). Free to use, copy, change and share, including commercially.
