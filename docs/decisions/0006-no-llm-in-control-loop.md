# 0006. No language model in the control loop

- **Status:** Accepted
- **Date:** 2026-11-15

## Context

AI is part of the project's appeal, but the daemon changes settings as root on low-power machines.

## Decision

Tuning uses small statistical methods that run locally. A language model may only explain and suggest, is opt-in, and never applies changes.

## Why

A language model on a small laptop would cost more power than it saves, respond too slowly, and could confidently suggest harmful commands.

## Consequences

The 'AI' features users see are mostly explanations. Suggestions from the model need human review before they become settings.

## Alternatives considered

A local model choosing settings directly. A cloud model in the loop (also needs network in the daemon, which the architecture forbids).
