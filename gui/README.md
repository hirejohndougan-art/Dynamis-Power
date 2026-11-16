# gui

The Dynamis Power app: Tauri 2 + React + TypeScript. It talks to the Dynamis Power service (`dynamisd`) only over D-Bus, opens on demand, and costs nothing when closed: there is no tray icon or background process, and while open it asks for status every 5 seconds only when the window is visible.

## What it has today

| Screen | Contents |
| --- | --- |
| Welcome | First run only, four steps: what it does; a check of this laptop; other power tools found (TLP, auto-cpufreq, TuneD, …) with *Switch off*, which asks for an administrator password and is undone when Dynamis Power is removed; and the choice of auto mode and learning. Nothing changes until *Start* |
| Home | Battery time left, the power mode (Longer battery / Balanced / Faster) and *Choose automatically*, live power use with a chart, the learning card, the last change with Undo, *Restore original settings*, and a safety strip for safe mode or a test in progress |
| Modes | The three modes with *Use this*; auto mode's preferences: mode when plugged in and on battery, the low-battery level, full speed for heavy work when plugged in; *Show Dynamis Power in the panel* |
| Apps | Battery use by app today and this week (estimates), what is open and in front, per-app options (never/always slow, a mode while open), and switches for priority in front and slowing background apps on battery |
| Battery | Forecast at your usual use and usual unplug time; *Make it last* (until a time); *Follow my routine* charging where supported; health, capacity, charge cycles, charge limit (where the battery supports one), and what the screen costs |
| Activity | Plain-language history with filters (automatic, mine, tests, rollbacks, problems) and Undo; a Logs tab with the full journal and search; **Export logs** with sections, time range, JSON or text, *Remove personal details*, a preview and *Copy summary* |
| Advanced | Settings (plain values, risk badges, status, Change and Reset, technical IDs behind *Details*, an extra confirmation for risky settings), Learning (every test, Forget, forget everything), Safety (safe mode, what is not allowed on this laptop, Restore) |

Not yet (see [../docs/UI.md](../docs/UI.md)): Sharing (phase 8), and the Profiles, Sensors, System and Developer tabs of Advanced. The design's five-step slider is three modes today, matching the service's three profiles.

## Layout

```
gui/
  src/
    backend/        types.ts (what the service sends), tauri.ts (real), mock.ts (stand-in for browser and tests)
    i18n/           en.ts (every string the app shows), index.ts (t, tn, plain value names)
    screens/        Welcome, Home, Modes, Apps, Battery, Activity, Advanced
    activity.ts     journal entries → plain-language items (pure, tested)
    exporter.ts     export files and the privacy filter (pure, tested)
    components.tsx  Switch, Dialog, Sparkline, Logo
    hooks.ts        status polling (visible window only), loading, actions
  src-tauri/        the native shell: thin commands over daemon/crates/dynamis-client
```

The shell stays thin on purpose: everything it does is one call into `dynamis-client`, which is tested against the real service in `daemon/crates/dynamisd/tests/dbus.rs`.

## Developing

You need Node 22 and, for the native app, Rust and WebKitGTK (`sudo apt install libwebkit2gtk-4.1-dev librsvg2-dev`).

```sh
cd gui
npm install
npm run dev          # the app in a browser, with the stand-in backend (no service needed)
npm test             # app tests (vitest)
npx tauri dev        # the real app window, talking to the installed service
npx tauri build --bundles deb   # an installable .deb in src-tauri/target/release/bundle/deb/
```

To run the real app against a **test** service instead of your laptop, start `dynamisd --session --root <fixture copy>` on a private bus (see [../daemon/README.md](../daemon/README.md)) and run the app with `DYNAMIS_BUS=session` and that bus's `DBUS_SESSION_BUS_ADDRESS`.

CI builds the `.deb` on every push; download it from the run's *Artifacts*.

## Rules

From [../CLAUDE.md](../CLAUDE.md#interface-rules), and how this code follows them:

- **Every string** comes from `src/i18n/en.ts` through `t()`. Setting values are shown in plain words (`valueName`); technical IDs and raw values only behind *Details*.
- **Undo** on every change that can be undone; **Restore original settings** on Home and in Advanced → Safety.
- **Risky settings** need a second, explicit yes.
- **Light and dark** follow the desktop (`prefers-color-scheme`); colours are tokens in `src/styles.css`.
- **Keyboard and screen readers:** real buttons, `role="switch"` and `radiogroup`, labelled dialogs and tabs, visible focus.

## Translating

Copy `src/i18n/en.ts` to `xx.ts`, translate the values (never the keys), and add it to `catalogs` in `src/i18n/index.ts`. Known gap: setting names and messages that come from the service (for example error sentences) are English for now.
