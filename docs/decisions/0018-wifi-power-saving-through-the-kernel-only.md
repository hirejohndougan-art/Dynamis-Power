# 0018: Wi-Fi power saving through a kernel-only exception, still no network access

**Status:** Accepted

## Context

Wi-Fi power saving is switched on and off through the kernel's wireless interface (nl80211), which is reached through a netlink socket, not through a file. The service's unit has `PrivateNetwork=yes`, `IPAddressDeny=any` and `RestrictAddressFamilies=AF_UNIX`, so it cannot see the Wi-Fi device at all. Safety rule 6 says the service has no network access. A netlink socket talks only to the local kernel; it cannot send or receive network traffic.

## Decision

- Allow `AF_NETLINK` in the unit and drop `PrivateNetwork=yes` (a private network namespace hides the Wi-Fi device). `IPAddressDeny=any` stays, and `AF_INET` and `AF_INET6` stay forbidden, so the service still cannot open a network connection of any kind.
- Only the nl80211 generic-netlink family is used, from one small module, for one request: set or read the power-save state of a Wi-Fi interface. No scanning, no connecting, no reading network names.
- `wifi.power_save` is a setting plugin like any other: probed, journaled, put back by Restore, blocked per laptop. The built-in quirk list skips chips known to drop connections or slow down with it on (Realtek RTL8821CE and similar).
- A separate helper program was the alternative; it would add a second privileged process, its own D-Bus interface and its own packaging, for the same single request.

## Consequences

- Rule 6 now reads "no network access: no network connections of any kind; the only exception is netlink to the local kernel, for Wi-Fi power saving." SAFETY.md says so.
- The service can see network interface names (not traffic, addresses or network names). Nothing about them is stored or shared.
- A test checks that the unit still forbids IP sockets and that the module sends only the power-save request.
