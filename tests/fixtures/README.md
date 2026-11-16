# Test fixtures

Tests never read or write the real `/sys` or `/proc`. Instead they point the daemon at a directory tree that mimics them. Every sensor and setting plugin takes a root path (`/` in production, a fixture directory in tests).

## Layout

```
tests/fixtures/
  sysfs/
    <machine-name>/          one directory per simulated machine
      MACHINE.md             what this fixture represents and where its values came from
      sys/...                files mirroring /sys paths
      proc/...               files mirroring /proc paths
```

## Rules

- **Label the source.** Each fixture's `MACHINE.md` says whether its values are synthetic or captured from a real machine (and which kernel).
- **No personal data.** Captured fixtures must have serial numbers, MAC addresses and usernames removed.
- **Small.** Include only the files the tests need.
- **Simulating changes over time** (a battery draining, a temperature rising) is done in test code, not with many fixture copies.

For device-level simulation beyond plain files (udev events, hotplug), the project plans to use [umockdev](https://github.com/martinpitt/umockdev).

## Available fixtures

| Fixture | Source |
| --- | --- |
| [synthetic-laptop](sysfs/synthetic-laptop/MACHINE.md) | Synthetic: a minimal two-core laptop on battery |
