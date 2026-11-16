# daemon

The Dynamis Power system service, written in Rust. Not started yet; it begins in phase 1 with the measurement logger and hardware inventory.

Planned parts:

- **Sensor plugins** read power, stall pressure, temperature, idle states and per-app use.
- **Setting plugins** each own one lever and implement probe, read, apply and revert, with a declared risk tier.
- **Fast loop** chooses the profile from context.
- **Learner** runs calibration and optimisation.
- **Journal** records every change before it is applied.
- **D-Bus API** serves `org.freedesktop.UPower.PowerProfiles`, `net.hadess.PowerProfiles` and an extended Dynamis interface.

All tests run against simulated hardware in `../tests/fixtures/`, never the real `/sys`.
