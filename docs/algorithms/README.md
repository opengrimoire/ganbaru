# Algorithms

This directory specifies deterministic domain decisions that must stay consistent across UI, Rust services, recovery, imports, and tests. Algorithm documents explain inputs, outputs, ordering, edge cases, and rationale. They do not mirror helper functions or persistence DDL.

## Calendar

- [Calendar algorithms](calendar/README.md)
- [Recurrence expansion](calendar/recurrence-expansion.md)
- [Time conflict detection](calendar/time-conflict-detection.md)

## Pomodoro

- [Pomodoro algorithms](pomodoro/README.md)
- [Focus authority and evidence](pomodoro/focus-authority.md)
- [State machine](pomodoro/state-machine.md)
- [Plan and history](pomodoro/plan-and-history.md)
- [Idle detection](pomodoro/idle-detection.md)
- [Adaptive policy](pomodoro/adaptive-policy.md)
- [Adaptive experiments](pomodoro/adaptive-experiments.md)

## Sync

- [Sync merge rules](sync/README.md)

## Reading algorithm status

These documents describe the intended product contract. A note labeled implementation gap identifies a known divergence in current code. Do not remove a desired invariant merely to match an accidental implementation detail: resolve the product decision, update code and tests, then remove the gap note.

Data ownership and persistence are indexed in [Data documentation](../data/README.md). Feature documents own user-visible workflows and copy.

## Writing algorithm specifications

A durable algorithm document should include:

- Normalized inputs and outputs.
- Deterministic ordering and tie-breakers.
- Time, timezone, and boundary semantics.
- Hard resource bounds.
- Persistence effects only where they are part of correctness.
- Examples that cover non-obvious cases.
- Known implementation or conformance gaps.

Avoid performance claims without measurements and exact source inventories that become stale after ordinary refactors.
