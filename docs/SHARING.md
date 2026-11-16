# Sharing results

Users can optionally share what worked on their laptop, so others with the same model start from proven settings. Sharing is off by default, and the app shows every field before anything is sent. See [../PRIVACY.md](../PRIVACY.md).

## Flow

```
 App (user clicks "Share")
   └─ shows exactly what will be sent
        └─ dynamis-sync uploads over HTTPS
             └─ Submission service (Cloudflare Worker)
                  ├─ validates against schema/v1 (rejects unknown fields)
                  ├─ rate-limits per anonymous install ID
                  └─ stores in database (Cloudflare D1)
                       └─ nightly job
                            ├─ combines submissions per laptop model, drops outliers
                            └─ publishes summaries to a public data repository
 App on another laptop
   └─ downloads its model's summary (no account needed)
        └─ uses it as a starting point, then re-verifies locally
```

No GitHub account is needed to submit or download. The summaries are public and auditable; raw submissions are not published.

## The results format

Defined by JSON Schema in [`../schema/v1/results.schema.json`](../schema/v1/results.schema.json), with an example in [`../schema/v1/examples/`](../schema/v1/examples/).

Key rules:

- **`schema_version`** is required. The service accepts only versions it knows.
- **No extra fields.** Every object sets `additionalProperties: false`, so nothing unexpected can be sent.
- **No identifying data.** No serial numbers, usernames, app names, usage times or locations. Dates are day-precision only. Memory and storage sizes are rounded.
- **Setting IDs** come from [SETTINGS.md](SETTINGS.md) and are permanent.

## Changing the format

1. Never edit a published version. Copy `schema/v1/` to `schema/v2/` and change the copy.
2. Bump `schema_version` and document the change in the schema's `$comment` and in [../CHANGELOG.md](../CHANGELOG.md).
3. The service must keep accepting the previous version until old app releases are no longer in use.
4. Add a migration so v1 records can be read as v2 when summaries are built.

## Abuse protection

Strict schema validation, per-install rate limits, outlier rejection when combining, a minimum number of independent reports before a result appears in a summary, and local re-verification on every machine. A shared result can never be applied without the receiving machine confirming it.

## Deletion

Each submission carries a random anonymous install ID. The app's "Delete my submissions" button asks the service to delete every record with that ID. Summaries rebuilt afterwards no longer include them.

## Status

Planned for phase 8. The schema exists now so that the app's internal experiment records use the same shape from the start.
