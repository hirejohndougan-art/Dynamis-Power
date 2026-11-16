# Auto-mode scenarios

The phase 3 gate: scripted situations with the profile auto mode must choose. Each file is run by `daemon/crates/dynamis-core/tests/auto_scenarios.rs` (`cargo test` in `daemon/`).

Format:

```json
{
  "name": "Short name",
  "about": "What this checks, in a sentence or two",
  "prefs": { "on_battery": "power-saver" },
  "available": ["power-saver", "balanced", "performance"],
  "steps": [
    { "t": 0,  "set": { "on_ac": true, "battery_pct": 80 }, "expect": "balanced" },
    { "t": 30, "set": { "on_ac": false }, "expect": "balanced", "workload": "light_interactive" }
  ],
  "max_switches": 3
}
```

- `t` is seconds since the start. `set` changes only the readings it names; the rest stay as they were.
- Readings: `on_ac`, `battery_pct`, `cpu_busy_pct`, `net_mb_s`, `max_temp_c`, `mic_in_use`, `media_playing`, `screen_inhibited`, `game_running`, `screen_locked`, `user_idle`.
- `expect` is the active profile after that step. `workload` (optional) is the detected workload class.
- `prefs`, `available` and `max_switches` are optional. Defaults are in `daemon/crates/dynamis-core/src/auto.rs`.
- `"repeat": {"every": 70, "times": 100, "toggle": "media_playing"}` on a step repeats it, flipping one reading each time (for flapping tests).

To add a case: copy a file, change it, run `cargo test`.
