# Contributing to Dynamis Power

Thanks for your interest. The project is at an early stage, so the most valuable contributions right now are hardware reports, ideas and careful review.

## Ways to help

- **Hardware reports.** Open a [hardware report](../../issues/new?template=hardware_report.yml) with your laptop model and what your system exposes. This helps us support more machines.
- **Quirk reports.** If a setting misbehaves on your hardware, file a [quirk report](../../issues/new?template=quirk_report.yml). These feed the safety blocklist.
- **Bug reports and feature requests.** Use the issue templates.
- **Code.** Pick an issue labelled `good first issue`, or open an issue to discuss a larger change before starting.

## Ground rules for code

1. **Read [docs/SAFETY.md](docs/SAFETY.md) first.** Any code that changes a system setting must follow it.
2. **Write code fresh.** Don't copy code from TLP, TuneD, auto-cpufreq or power-profiles-daemon. They are GPL-licensed, and this project is MIT.
3. **Test against fixtures, never the real system.** Tests use simulated hardware under `tests/fixtures/`. A test that reads or writes the real `/sys` will be rejected.
4. **Plain language in the app.** User-facing text must make sense to someone who has never used a terminal, and must go through the translation layer.
5. **Version every saved format.** Changes to learned settings, the journal or shared results need a version bump and a migration.
6. **One logical change per pull request**, with a clear description of what changed and why.

## Rust style

Run `cargo fmt` and `cargo clippy -- -D warnings` before pushing. Avoid `unsafe`; if it's truly needed, explain why in a comment.

## Licence

By contributing, you agree that your contributions are released under the [MIT License](LICENSE).

## Conduct

Everyone taking part must follow the [Code of Conduct](CODE_OF_CONDUCT.md).
