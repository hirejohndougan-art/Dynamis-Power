# Maintaining Dynamis Power

This document is for whoever looks after the project, today or in future. It should be possible to hand the project to a new maintainer using only this repository.

## Responsibilities

| Area | What it involves |
| --- | --- |
| Pull requests | Review for safety (see [SAFETY.md](SAFETY.md)), tests against fixtures, and matching documentation |
| Quirk reports | Triage reports of misbehaving settings and update the signed quirk list |
| Releases | Version, changelog, signed packages |
| Shared-results service | Keep the service running, watch for abuse, rebuild summaries |
| Keys and accounts | Look after signing keys and service accounts, and hand them over properly |

## Review checklist

- [ ] Changes to system settings go through a setting plugin with probe, read, apply, revert and a risk tier.
- [ ] Every change is journaled before it is applied.
- [ ] Tests use fixtures, never the real `/sys` or `/proc`.
- [ ] No code copied from GPL projects.
- [ ] User-facing text is plain language and goes through translations.
- [ ] Saved formats changed? Version bumped and migration added.
- [ ] New setting? Added to [SETTINGS.md](SETTINGS.md) with a permanent ID.
- [ ] Docs and [CHANGELOG.md](../CHANGELOG.md) updated in the same pull request.

## Quirk triage

1. A quirk report names a laptop model, setting ID, value and symptom.
2. Reproduce if possible, or look for matching reports.
3. Add an entry to the quirk list blocking that setting value for that model (and kernel range, if known).
4. Sign and publish the updated list (below). Installations pick it up within a day, without an app update.
5. If the problem is severe and widespread, also lower the setting's default risk tier in the next release, or add it to the built-in list (`daemon/crates/dynamis-core/quirks/builtin.json`).

## The quirk list

Format and folder: [quirks/README.md](../quirks/README.md). Design: [decision 0009](decisions/0009-signed-quirk-list.md).

**Once, to set up signing** (on your own computer, not a server):

```sh
cd daemon
cargo run -p dynamis-quirktool -- keygen ~/dynamis-quirk-signing.key
```

It prints a public key. Add it to `TRUSTED_KEYS` in `crates/dynamis-core/src/quirks.rs` and release. Back the secret key file up somewhere offline. **Never commit it.** Making a second key, kept offline, and adding both public keys means losing one is not an emergency.

**Each time you publish:**

1. Edit `quirks/quirks.json`. Increase `serial` by one. Give each new quirk a new `id`.
2. `cargo run -p dynamis-quirktool -- sign ../quirks/quirks.json ~/dynamis-quirk-signing.key` (it refuses lists that name unknown settings).
3. `cargo run -p dynamis-quirktool -- verify ../quirks/quirks.json <public key>`.
4. Commit `quirks.json` and `quirks.json.sig` together and merge to `main`.

A kill switch for a setting everywhere is a quirk with an empty `match` and no `values`.

## Releases

1. Update [CHANGELOG.md](../CHANGELOG.md) under a new version heading.
2. Bump versions using [semantic versioning](https://semver.org): breaking changes to D-Bus, config or saved formats are a major bump.
3. Tag the release (`vX.Y.Z`) and build packages.
4. Sign packages and publish checksums.
5. New setting plugins go to the beta channel first and move to stable only after enough clean reports.

## Keys and accounts

| Item | Used for | Where it should live |
| --- | --- | --- |
| Release signing key | Signing packages | Offline, with a backup; public key in the repository (to be added) |
| Quirk list signing key | Signing the quirk feed | Separate from the release key; public key compiled into the app |
| Submission service account | Cloudflare Worker and D1 | Owned by a project account, not a personal one, with a second admin |
| GitHub repository | Code and data | At least two people with admin rights once there is a team |

Losing a signing key means shipping a new public key in an app update and rotating. Never store private keys in the repository.

## Handing over

1. Add the new maintainer as a repository admin.
2. Transfer or share service accounts and signing keys securely.
3. Update [SECURITY.md](../SECURITY.md) contacts.
4. Announce the change in a pinned issue.

## Bus factor

Until there are at least two maintainers, document anything that exists only in one person's head here or in [decisions/](decisions/).
