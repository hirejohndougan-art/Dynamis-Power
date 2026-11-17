# 0005. Cloudflare Worker and D1 for shared results

- **Status:** Superseded by [0028](0028-sharing-through-github.md)
- **Date:** 2026-11-15

## Context

Users must be able to share results with one click, without an account. The project has no budget for servers.

## Decision

Submissions go to a Cloudflare Worker that validates them and stores them in Cloudflare D1. A nightly job publishes per-model summaries to a public repository.

## Why

No account needed to share or download; free tier fits the expected scale; no server to maintain; summaries stay public and auditable.

## Consequences

Depends on one provider; keep the Worker small and the schema provider-neutral so it can move. Free-tier limits may change.

## Alternatives considered

GitHub pull requests (free and transparent, but needs a GitHub account to submit). Supabase (good dashboard, but free projects pause when idle). Self-hosting (free, but depends on home internet and exposes the home network).
