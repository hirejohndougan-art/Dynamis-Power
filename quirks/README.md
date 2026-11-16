# Published quirk list

This folder is where the **signed, downloadable** quirk list lives once the project has a signing key. Every installation's `dynamis-sync` helper fetches `quirks.json` and `quirks.json.sig` from here on the `main` branch, once a day.

- Nothing is published yet, so installations use only the list built into the program (`daemon/crates/dynamis-core/quirks/builtin.json`).
- A list is used only if its signature matches a key in `TRUSTED_KEYS` (`daemon/crates/dynamis-core/src/quirks.rs`), and only if its `serial` is not lower than one already accepted.
- How to sign and publish: [docs/MAINTAINING.md](../docs/MAINTAINING.md#the-quirk-list).

Format (version 1):

```json
{
  "v": 1,
  "serial": 2,
  "issued": "2026-12-01",
  "quirks": [
    {
      "id": "q-0002",
      "setting": "vm.dirty_writeback",
      "values": ["6000"],
      "match": { "vendor": "Example*", "model": "*Book 14*", "kernel": "6.14*" },
      "reason": "Plain-language reason shown to people",
      "url": "https://github.com/hirejohndougan-art/Dynamis-Power/issues/123"
    }
  ]
}
```

`match` fields (all optional, all must match; `*` matches anything, case is ignored): `vendor`, `model`, `cpu`, `wifi_driver`, `kernel`. An empty `match` applies to every machine: that is the kill switch for a setting. Leaving out `values` blocks every change to the setting.
