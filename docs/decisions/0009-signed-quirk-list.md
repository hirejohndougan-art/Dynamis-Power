# 0009. A signed quirk list, downloaded without root

- **Status:** Accepted
- **Date:** 2026-11-16

## Context

Some settings break particular laptops. The project needs a way to stop every installation from applying a known-bad setting within a day, without an app update and without giving the root service network access (safety rule 6).

## Decision

- A **built-in** list ships inside the program. A **downloaded** list, published in this repository's `quirks/` folder, adds to it.
- The downloaded list carries a detached **ed25519** signature. The public keys allowed to sign are compiled into the program (`TRUSTED_KEYS`). The private key never goes in the repository.
- `dynamis-sync` downloads the list once a day, as a throwaway unprivileged user (systemd `DynamicUser=`), over HTTPS only, at most 1 MB. It refuses to run as root.
- The root engine reads the helper's files and verifies the signature again **before parsing** anything. A list with a lower serial than one already accepted is ignored, so an old list cannot be replayed to lift a newer block.
- A quirk blocks a change before it is journaled or written. When the engine opens and finds a setting already set to a value a new quirk blocks, it puts the original value back and journals `quirk`. That is the kill switch.
- Until the maintainer creates a key, `TRUSTED_KEYS` is empty and downloaded lists are ignored.

## Why

Two separate checks (helper and engine) mean a bug in one is not enough to use a bad list. Keeping the network out of the root process means a compromised download path can, at worst, fail to deliver a list; it cannot change a setting. ed25519 has small keys and signatures and a well-reviewed Rust implementation.

## Consequences

- Losing the signing key means shipping a release with a new public key. Keeping a second, offline backup key in `TRUSTED_KEYS` avoids that.
- The list is public and hosted on GitHub; if GitHub is unreachable, installations keep the last good list.
- Matching uses what the inventory reads (vendor, model, CPU, Wi-Fi driver, kernel), so a quirk cannot target hardware the inventory does not see yet.
