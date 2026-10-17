# Adaptive Pomodoro experiments

The adaptive engine has seven bounded run-level experiment lanes. Each lane compares a control rhythm with one treatment under narrow eligibility. Assignment, context, selected values, outcomes, analysis, terminal state, and cooldown are persisted locally. Shared objectives, statistics, and guardrails are defined in [Adaptive policy](adaptive-policy.md).

## Rhythm notation

`focus / short break / long break / cadence`, in minutes. For example, 40/5/10/C4 means 40 minute focus, 5 minute short break, 10 minute long break, and a long break after every fourth focus.

## Experiment catalog

| Lane | Control | Treatment | Narrow eligibility | Primary evidence | Main guardrails |
| --- | --- | --- | --- | --- | --- |
| Focus duration | 40 minute focus | 45 minute focus | Policy already selects capacity growth, comparable history exists, and strain, recovery debt, avoidance, blocker, and break-drift risk are low | Conservative clean-focus gain | Completion, stop rate, blocker pressure, missed planned work, next-day return |
| Short-break duration | 5 minutes | 7 minutes | Repeated low-risk short-break return drift without broader recovery or blocker pressure | Reduced short-break overtime | Completion, blockers, skipped breaks, missed planned work, next-day return, no worse drift |
| Long-break duration | 10 minutes | 15 minutes | Clean long-break return drift, low broader risk, and no blocked attempt during long-break overtime | Reduced long-break overtime | Completion, blockers, skipped breaks, missed planned work, next-day return, no worse drift |
| Earlier long-break cadence | C4 | C3 | Mild late-cycle recovery pressure after several focus positions without high strain, recovery debt, or avoidance | Reduced blocker pressure or conservative completion improvement | Clean focus, completion, skipped breaks, missed planned work, next-day return |
| Later long-break cadence | C4 | C5 | Very clean high-momentum work, at least 12 completed focus periods, completed breaks, no skipped breaks, blocked attempts, break drift, or focus failures, and enough comparable history | Conservative clean-focus gain | Completion, blockers, skipped breaks, long-break return, missed planned work, next-day return |
| Focus plus short-break bundle | 40/5/10/C4 | 45/7/10/C4 | Strong clean momentum, stable short-break return, substantial completed focus and break history, comparable evidence, and low risk | Conservative clean-focus gain | Completion, stop rate, blockers, skipped breaks, short-break drift, missed planned work, next-day return |
| Long-recovery bundle | 40/5/15/C4 | 40/5/15/C3 | Clean long-break drift, stable completed focus and break history, comparable evidence, no blocked attempt during overtime, and low risk | Reduced blocker pressure, reduced long-break drift, or conservative completion improvement | Clean focus, completion, stop rate, blockers, skipped breaks, missed planned work, next-day return |

## Shared exclusions

No experiment starts when adaptive mode is off, the lane is in cooldown, the posture is fallback, recovery, or guardrail, history quality is insufficient, the treatment would exceed the safe range, current configuration does not match the lane's control family, or a required outcome could only be observed with disallowed data. Failing eligibility keeps the current safe rhythm and records no control observation.

## Assignment lifecycle

1. Build a coarse context and policy snapshot from local history.
2. Select at most one eligible lane in fixed priority order: focus plus short-break bundle, focus duration, short break, long-recovery bundle, long break, earlier cadence, later cadence. Bundles precede their scalar alternatives.
3. Reuse an existing assignment for the same command or run.
4. Otherwise assign control or treatment from the persisted seed and exploration balance. Exploration is limited per context within a rolling seven-day window.
5. Commit assignment, run snapshot, chosen values, and first segment atomically.
6. Attach phase, run, same-day, and next-day outcomes as each matures.
7. Aggregate by experiment, variant, and coarse context.
8. Persist terminal result and cooldown without a synthetic extra assignment.

Component and bundle experiments are distinct evidence. A bundle result makes no causal claim about its components.

## Outcome maturity

Phase outcomes (completion, stop, focus failure, clean focus, break skip, break overtime, blocker pressure) attach immediately. Run outcomes attach after the run closes. Same-day missed work and blocker pressure attach once that local day can no longer change. Next-day return attaches only after its observation window passes. Missing data stays unknown; a user who does not open the app the next day is judged only by the explicit next-day rule, not automatically as harm. Outcome writers are idempotent, so re-running maturation never inflates sample size.

## Lane-specific result rules

- **Focus duration:** 45 minutes wins only with conservative clean-focus gain and preserved completion and next-day behavior. Any completion decline or increase in stops, blockers, missed work, or avoidance returns to 40.
- **Short break:** 7 minutes wins only when it materially reduces short-break overtime without more skipped breaks or broader risk. More break time without better return is not a win.
- **Long break:** 15 minutes wins only when it reduces long-break overtime while preserving later completion and return. More drift or blocker pressure loses.
- **Earlier cadence:** C3 wins when earlier recovery conservatively reduces late-cycle blocker pressure or improves completion without sacrificing clean focus. Increased skips, drift, missed work, or avoidance loses.
- **Later cadence:** C5 wins only when delaying long recovery increases clean focus in already stable high-momentum contexts without degrading any guardrail.
- **Focus plus short-break bundle:** 45/7 must improve clean focus while the longer short break keeps drift in check. Harm applies to the combined shape only.
- **Long-recovery bundle:** C3 within the 15 minute long-break rhythm must reduce blocker pressure or long-break drift, or improve completion, while preserving clean focus. Harm blocks only the combined shape.

## Terminal states

Treatment preference records completed; guardrail-forced control records abandoned. An inconclusive lane stays active only while eligibility and exploration budget allow more observations. Cooldown is 14 days per lane. Leaving adaptive mode stops new assignments; returning later respects the current version and any cooldown.

## Diagnostic replay

Replay can explain a prior assignment or evaluate a candidate version from persisted snapshots. It uses the historical version unless explicitly evaluating a candidate, reads assignments and outcomes without rewriting them, produces bounded reason codes, evidence tier, and guardrail results, avoids diary, Notes, Chat, and Calendar title content, and keeps historical explanations distinct from hypothetical results. Replay never changes the rhythm a historical run used.
