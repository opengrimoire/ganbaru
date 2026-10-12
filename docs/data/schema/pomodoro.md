# Pomodoro schema

Pomodoro persistence separates planned rhythm, one run's configuration snapshot, actual phases, pauses, lifecycle events, native execution state, and adaptive evidence. Later Calendar or preference edits therefore never rewrite history. Derivation rules are in [Plan and history](../../algorithms/pomodoro/plan-and-history.md).

## Tables

| Group | Tables |
| --- | --- |
| Configuration | `calendar_event_pomodoro_configs`, `calendar_event_pomodoro_config_count_rhythms`, `calendar_event_pomodoro_config_sequence_steps` |
| Run history | `pomodoro_runs`, `pomodoro_run_count_rhythms`, `pomodoro_run_sequence_steps`, `pomodoro_segments`, `pomodoro_pauses`, `pomodoro_run_events`, `pomodoro_run_adaptive_snapshots` |
| Native execution | `pomodoro_execution_state` (one revisioned accepted state), `pomodoro_execution_receipts` (command ID, request, and result) |
| Adaptive | `pomodoro_adaptive_*` policies, bounds, experiments, variants, assignments, outcomes, decisions with values, reasons, and state scores, context snapshots and states, data quality flags, and planned blocks |
| Calendar archive | `calendar_event_archive_pomodoro_*` copies of Pomodoro configuration kept with archived Calendar events |

## Event configuration

An eligible Calendar event has one Pomodoro configuration. Count rhythms define focus, short break, long break, and long-break cadence. Sequence rhythms define a bounded repeating sequence. Replacing a rhythm validates the complete new shape before transactionally replacing its child rows.

The optional idle timeout is positive when present. Null disables idle detection; zero is invalid.

Configuration is future intent, never the source of truth for a phase that already started.

## Runs

A run is the durable session header. It records the concrete event, original recurring-series identity, event date and planned window, actual start and end, end reason, start trigger, title snapshot, rhythm snapshot, idle setting, heartbeat, and inherited focus and phase progress. Inherited state is copied onto the new run rather than recomputed through a chain of older runs, so a run stays self-contained after earlier Calendar rows are archived.

Calendar edits may retarget a run through separate current-reference columns (`current_occurrence_id`, `current_event_date`, `current_event_title`). Original provenance is never rewritten. History reads resolve the current reference, falling back to the original occurrence identity, in the same snapshot as segments and pauses, and oversized identity, segment, or pause sets fail explicitly instead of producing a truncated rail.

Recurrence protection uses original home-zone occurrence identities, not the run's device-local planning date. The two can differ around midnight and must not be substituted for each other.

A partial unique constraint permits at most one globally active segment. Multiple runs may refer to the same event and are never merged by title, time range, or event identity. A cross-block transition or reconfiguration closes the current run and starts a new one.

## Segments and pauses

Segments represent phases that actually started; the future plan is never pre-created. A segment records run, phase, rhythm position, planned and actual boundaries, status, and chosen duration. Completed and interrupted rows are immutable. Planned UI bands are projections.

A pause belongs to one segment, with a reason, a start, and a null end only while open. Pauses cannot start before their segment, extend past a closed segment, or overlap. Paused time is not progress.

## Run events

Run events are append-only explanations for lifecycle decisions not fully expressed by segment rows, such as skip, extension, reconfiguration, transition, stop, completion, and recovery. They are audit evidence, not canonical state. Command receipts and state preconditions prevent duplicate semantic events on retry.

## Native execution

`pomodoro_execution_state` holds the accepted execution snapshot with a revision. Every command validates the expected revision and stores its receipt in the same transaction as the canonical rows it changes, so a retried command returns the original result. Recovery rules are in [Focus authority](../../algorithms/pomodoro/focus-authority.md#recovery); the heartbeat bounds desktop recovery and is not an activity log. Recovery is idempotent.

## Adaptive data

Adaptive Pomodoro is local and explicit opt-in. Storage separates:

- Context and policy snapshots.
- Phase or run boundary decisions.
- Experiment definitions and bounded variants.
- Deterministic assignments.
- Later outcomes and aggregate evidence.
- Terminal experiment state and cooldown.

A decision records the previous and selected value, reason codes, state scores, policy version, and target phase or run. It commits in the same transaction that starts the affected segment or run. Outcomes are appended later and never rewrite the decision. See [Adaptive policy](../../algorithms/pomodoro/adaptive-policy.md) and [Adaptive experiments](../../algorithms/pomodoro/adaptive-experiments.md).

## Time and deletion

Timestamps use normalized text within this domain. Durations derive from instants and closed pauses, not local clock labels.

Calendar deletion and recurrence edits preserve referenced history, and a run stores enough event context to remain interpretable after archive. Cleanup may rebuild derived adaptive aggregates but never silently deletes canonical segments, pauses, decisions, or outcomes in retention scope.
