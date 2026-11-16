# 0007. Plain files for the change journal

- **Status:** Accepted
- **Date:** 2026-11-15

## Context

The change journal must survive the exact failures it exists to diagnose: a frozen desktop, a hard reset, a power cut in the middle of a write. It must be readable by a person with nothing but a terminal, and by the app later. ARCHITECTURE.md originally planned one SQLite database for samples, learned data and the journal.

## Decision

The safety state lives in small plain files in `/var/lib/dynamis-power/`, separate from any future database:

| File | Contents |
| --- | --- |
| `journal.jsonl` | One JSON object per line, appended and flushed to disk (`fsync`) before each kernel write |
| `pending.json` | The change in flight; its presence at start-up means the last change never finished |
| `baseline.json` | Each setting's original value, recorded the first time it is changed and never overwritten |
| `blocked.json` | Settings blocked after an unfinished or failed change, with the reason |
| `lock` | Kernel file lock (`flock`) held while changing settings; contains the holder's PID as a hint ([0008](0008-standard-profiles-through-the-engine.md)) |

Every record carries `"v": 1`. The JSON files are written to a temporary file and renamed into place, so a crash leaves either the old file or the new one.

Samples and learned data may still go into SQLite later; that is a separate decision.

## Why

- Appending one line and calling `fsync` is the simplest write that survives a hard reset. A torn last line is skipped on reading and loses at most that one entry.
- `cat`, `tail` and `jq` can read it during a support request without any tool from this project.
- No database library in the root process for the most safety-critical path.

## Consequences

- The journal grows without bound. Rotation (keep the last N MB, never during a pending change) is needed before release.
- Queries across the journal are linear scans. At a few lines per change that is fine for years of use.
- Format changes bump `v` and need a reader for every older version, like the log format.
