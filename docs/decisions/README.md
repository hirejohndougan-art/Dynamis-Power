# Decision records

Each file records one significant decision: the context, what was decided, why, and what it costs. They exist so future maintainers and forks understand why the project is the way it is.

| Record | Decision |
| --- | --- |
| [0001](0001-rust-daemon.md) | Rust for the daemon |
| [0002](0002-tauri-react-gui.md) | Tauri and React for the app |
| [0003](0003-mit-licence.md) | MIT licence |
| [0004](0004-standard-power-profiles-interface.md) | Serve the standard power-profiles D-Bus interface |
| [0005](0005-sharing-service.md) | Cloudflare Worker and D1 for shared results |
| [0006](0006-no-llm-in-control-loop.md) | No language model in the control loop |
| [0007](0007-plain-file-change-journal.md) | Plain files for the change journal |
| [0008](0008-standard-profiles-through-the-engine.md) | Standard profiles go through the change engine |
| [0009](0009-signed-quirk-list.md) | A signed quirk list, downloaded without root |
| [0010](0010-no-systemd-watchdog-yet.md) | No systemd watchdog for now; rate limits arrive with automatic changes |

## Adding a record

Copy the most recent file, number it next in sequence, and set the status to **Proposed**. Change it to **Accepted** when merged. Never delete a record; to reverse a decision, write a new one and mark the old one **Superseded by NNNN**.
