# Sharing results

Users can optionally share what worked on their laptop, so others with the same model start from proven settings. Sharing is off by default, and the app shows every field before anything is sent. See [../PRIVACY.md](../PRIVACY.md).

## Flow

Decided in [decision 0028](decisions/0028-sharing-through-github.md) (which replaced the earlier Cloudflare plan, 0005).

```
 App (person clicks "Share my results")
   └─ shows exactly what will be sent, and that the GitHub account will be shown
        └─ opens the results repository's "Share results" issue form in the browser
           (results filled in, or on the clipboard when too long for a web address)
             └─ the person signs in to GitHub and submits
                  └─ GitHub Action on the new issue
                       ├─ validates against schema/v1 (rejects unknown fields)
                       ├─ account at least 30 days old; one account counts once per model
                       └─ commits submissions/<model>/<issue>.json, closes the issue
                            └─ nightly Action
                                 ├─ combines per laptop model (3 accounts or more), drops outliers
                                 └─ signs and publishes summaries/<model>.json
 Another laptop
   └─ dynamis-sync downloads its model's summary (no account needed)
        └─ checks the signature, then the service uses it as a starting point
           and re-verifies every setting locally
```

Submitting needs a GitHub account; downloading does not. Dynamis Power never handles a GitHub password or token: the person submits in their own browser. Submissions are public and show the account that made them.

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

Strict schema validation, accounts at least 30 days old, one account counting once per laptop model, outlier rejection when combining, at least 3 different accounts before a result appears in a summary, signed summaries, and local re-verification on every machine. A shared result can never be applied without the receiving machine confirming it.

## Deletion

Comment `/delete` on your own submission issue, or open a "Delete my results" issue in the results repository: an Action removes every file submitted from your account and rebuilds the affected summaries. The app's *Delete my submissions* button opens that page.

## Status

Planned for phase 8; how it works is decided (0028), nothing is built yet. The schema exists now so that the app's internal experiment records use the same shape from the start.
