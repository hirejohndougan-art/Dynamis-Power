# Security policy

Dynamis Power's daemon runs as root, so security problems are taken seriously.

## Reporting a vulnerability

Please **do not** open a public issue for security problems. Instead, use GitHub's private vulnerability reporting: go to the **Security** tab of this repository and choose **Report a vulnerability**.

Include what you found, how to reproduce it, and the versions affected. You will get an acknowledgement as soon as the maintainer can respond, and credit in the release notes if you'd like it.

## Supported versions

There are no releases yet. Once there are, only the latest release will receive security fixes.

## Design principles

- The root daemon is kept small and has no network access.
- Actions from the app require polkit authorisation.
- The daemon's D-Bus input is validated and fuzz-tested.
- Releases and the downloaded quirk list are signed.

See [docs/SAFETY.md](docs/SAFETY.md) for the full set of safety rails.
