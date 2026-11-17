# 0028. Sharing results through a public GitHub repository

- **Status:** Accepted
- **Date:** 2026-11-16
- **Supersedes:** [0005](0005-sharing-service.md)

## Context

Decision 0005 chose a Cloudflare Worker and database so that sharing needed no account. The project owner prefers to run nothing outside GitHub: no second provider, no server code to keep alive, no database to back up, and every step visible. The cost is that sharing needs a GitHub account. The owner accepted that cost (2026-11-16). Downloading what others shared still needs no account.

## Decision

1. **A public results repository** (`dynamis-power-results`, name to be confirmed when it is created) holds the submissions, the per-model summaries and the GitHub Actions that check and combine them.
2. **Submitting happens in the person's own browser.** *Share my results* in the app shows every field, then opens the repository's "Share results" issue form with the results already filled in. If the results are too long for a web address, they are copied to the clipboard and the form says where to paste them. The person signs in to GitHub and presses *Submit* themselves. Dynamis Power never sees or stores a GitHub password or token, and the service stays without network access (safety rule 6).
3. **An Action checks each submission**: it must match `schema/v1` exactly (no extra fields), the account must be at least 30 days old, and one account counts once per laptop model. A valid submission is committed as `submissions/<model>/<issue number>.json` and the issue is closed with a short reply; an invalid one gets a plain explanation.
4. **A nightly Action builds the summaries**: per laptop model, results from at least 3 different accounts, outliers dropped, signed with the project's key (the same scheme as the quirk list, decision 0009; the key is an Actions secret).
5. **Downloading** goes through the unprivileged `dynamis-sync` helper: it fetches the summary for this laptop's model from the repository and checks the signature before the service uses it. No account needed. A shared result is only a starting point: every setting is still tested on this laptop before it is kept.
6. **Deleting**: commenting `/delete` on one's own submission issue, or opening a "Delete my results" issue, makes an Action remove every file submitted from that account and rebuild the affected summaries. The app links to both.
7. **The install ID** stays in the format (schema v1 requires it) but is no longer needed for deletion; it lets the Action treat several submissions from one install as one report.

## Consequences

- Sharing needs a GitHub account; people without one can still use shared summaries.
- **A submission is public and shows the GitHub account that made it.** The results themselves contain nothing personal, but the account name is visible on the issue and in the commit. The app says so on the confirmation screen before the browser opens, and PRIVACY.md says so.
- Everything is auditable: anyone can read the Actions, every submission and every summary.
- GitHub's free Actions minutes and repository size limits apply; submissions are small JSON files.
- No server, no database, no second provider to depend on.

## Alternatives considered

- **Cloudflare Worker and D1 (0005):** no account needed, but a second provider and server code to maintain.
- **Signing in to GitHub from the app (device flow) and filing the issue by the API:** one click fewer, but the helper would hold a token able to write to the person's public repositories. Submitting in the browser keeps that power with the person.
- **Pull requests:** needs a fork and is harder for someone who has never used GitHub; an issue form is a single page.
