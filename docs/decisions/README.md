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
| [0011](0011-auto-mode-rules-and-session-agent.md) | Auto mode: plain rules, and a session agent that reports five facts |
| [0012](0012-calibration-method.md) | How calibration tests and keeps a setting |
| [0013](0013-app-shares-not-caps.md) | Apps get a bigger or smaller share, never a cap or a freeze |
| [0014](0014-tuning-together-one-step-at-a-time.md) | Tune interacting settings together, one step at a time |
| [0015](0015-prediction-by-feedback-and-habits.md) | "Last until" steers by feedback; habits are simple and kept in UTC |
| [0016](0016-mode-levers-without-epp.md) | Modes use turbo, the speed limit and the governor where there is no speed preference |
| [0017](0017-importing-a-tlp-configuration.md) | Importing TLP: battery values to Longer battery, mains values to Faster, shown first |
| [0018](0018-wifi-power-saving-through-the-kernel-only.md) | Wi-Fi power saving through netlink to the local kernel; still no network access |
| [0019](0019-running-from-battery-while-plugged-in.md) | Running from the battery while plugged in: opt-in, standard control only, recalibration first |
| [0020](0020-quiet-by-steady-power.md) | Learning judges "quiet" by steady power, with a processor limit scaled by cores |
| [0021](0021-unclean-shutdowns-always-count.md) | Every unclean shutdown counts towards safe mode |
| [0022](0022-other-power-tools.md) | Which power tools run alongside, and which to offer to switch off |
| [0023](0023-desktops-two-gpus-image-based-flatpak.md) | Desktops, two graphics chips, image-based systems and Flatpak |
| [0024](0024-gnome-extension-in-the-package.md) | The GNOME extension ships in the package |
| [0025](0025-hibernate-on-low-battery-is-advice.md) | Sleep that turns into hibernation is advice only |

## Adding a record

Copy the most recent file, number it next in sequence, and set the status to **Proposed**. Change it to **Accepted** when merged. Never delete a record; to reverse a decision, write a new one and mark the old one **Superseded by NNNN**.
