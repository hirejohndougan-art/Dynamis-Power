# 0011. Auto mode: plain rules, and a session agent that reports five facts

- **Status:** Accepted
- **Date:** 2026-11-16

## Context

Phase 3 makes Dynamis Power choose the profile on its own (auto-mode layers 1 and 2 in DESIGN.md). It needs to know what the laptop is doing. The root service can see the charger, battery, CPU, network, temperatures and whether a microphone is recording. It cannot see inside the desktop session: whether media is playing, the screen is locked, a full-screen video is asking the screen to stay on, or a game is running.

## Decision

- **Rules, not a model.** `auto.rs` is a short, ordered list of rules over eight workload classes, written as a pure function so every decision can be replayed. The phase 3 gate is a set of scenario files anyone can read and extend (`tests/scenarios/`).
- **Smoothing.** Hysteresis on every threshold, a 60-second dwell time between switches, and at most 12 non-urgent switches an hour (the rate limit promised in decision 0010). Urgent changes skip both: charger plugged in or out, critical battery, a call starting, the person coming back to an idle laptop.
- **People win.** Holds from apps still win over auto mode. A profile picked by hand pauses auto mode until the charger is plugged in or out, so the choice is not undone a minute later.
- **Off by default.** Auto mode is switched on with `dynamis auto on` (later, a switch in the app).
- **The session agent** (`dynamis-agent`) runs as the logged-in person and reports exactly five yes/no facts: media playing, screen locked, screen-dimming inhibited, game running, user idle (always no for now). It is event-driven: it sleeps until the desktop sends a signal and reports only when a fact changes. It sends no window titles, app names or media titles.
- **Readings every 15 seconds while auto mode is on**, every 60 while the laptop is idle. With auto mode off there is no timer at all.
- **The focused app is not tracked yet.** It needs desktop-specific code (KWin scripting on KDE Wayland) and matters most for per-app behaviour, so it moves to phase 6.

## Why

Rules a person can read are easier to trust and debug than a classifier, and the design already plans a learned layer later. Five facts are enough for the rules and reveal very little about the person.

## Consequences

- On a desktop without MPRIS, a screen locker on the ScreenSaver interface, KDE's PolicyAgent or GNOME's session manager, the agent reports "no" for what it cannot see, and auto mode falls back to system readings.
- The microphone check reads ALSA status files, so it sees calls on any desktop, but also anything else that records (a voice memo).
- Auto mode makes switches the person did not ask for, and every one is journaled with its reason (`auto: …`), so Activity and `dynamis journal` can explain it.
