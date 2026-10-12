# Adaptive Pomodoro policy

Adaptive Pomodoro is a local, explicit opt-in policy that adjusts bounded rhythm choices from the user's own history. Its goal is sustainable completed focus, not maximum timer length. It must not diagnose health, punish missed work, or turn noisy behavior into confident advice.

The policy is deterministic from persisted inputs, versioned, auditable, reversible, and applied only at run or phase boundaries.

**Status: Implemented.** `ganbaru-pomodoro` owns state scoring, the base policy, seven experiment lanes, bounded replay approval, snapshots, and transactional run and phase decisions. Physical platform acceptance remains open.

## Objective

The policy balances clean completed focus, completion and stop behavior, break return and skips, blocker pressure, planned-work completion, next-day willingness to return, and uncertainty from small samples.

More focus seconds are not automatically better. A treatment that lengthens one session but increases stops, avoidance, missed blocks, or break drift must not be preferred.

## Scope and consent

Analysis runs locally against the active vault; behavioral history is never uploaded and no cloud model is involved. Ordinary Pomodoro does not enable adaptation: the user must choose the adaptive preset. Custom rhythms stay user-authored. Leaving adaptive mode stops new assignments without rewriting historical decisions or outcomes.

The product presents adaptation as experimental assistance based on local behavior, never inferred energy, strain, or avoidance as medical fact.

## Safe range

Adaptive choices stay within fixed bounds regardless of evidence:

| Value | Range |
| --- | --- |
| Focus | 15 to 60 minutes in 5 minute steps (reductions stop at 25 unless recent focus failure justifies going lower) |
| Short break | 3 to 12 minutes |
| Long break | 10 to 30 minutes in 5 minute steps |
| Long-break cadence | Every 2 to 5 focus positions |

The active event end always clips the selected phase.

## Decision boundaries

Adaptation occurs only at run start, at a phase boundary before the next segment starts, or through an explicit reconfiguration. It never changes the duration of a running phase.

The decision, any experiment assignment, and the new run or segment commit in one transaction, so a crash cannot leave an unexplained duration. The initial run rhythm stays an immutable snapshot; later boundary choices are separate decisions, and recovery reads the latest accepted decision rather than reverting to the initial rhythm.

## Context

Context may include only validated local evidence: coarse time of day, session position and recent completed focus, event length and remaining window, current and recently used rhythm, recent stops, focus failures, skipped breaks, break overtime, blocker pressure, same-day missed planned blocks, next-day return, and optional user-entered energy or environment.

Context uses coarse categories so evidence is not fragmented and unnecessary detail is not exposed. Calendar titles, Notes, Chat, keystrokes, camera data, and arbitrary application activity are never features. Missing optional context stays unknown rather than being filled with a negative assumption.

## Policy postures

Before experimentation, the deterministic policy picks a broad posture, recorded as a decision kind (`fallback`, `recovery`, `guardrail`, `hold`, `explore`, or `exploit`):

- **Fallback:** confidence is too low or there is no comparable history. Keep the current rhythm.
- **Recovery and guardrail:** recent stops, focus failure, high blocker pressure, repeated missed work, break drift, or weak return. Prefer a shorter or more conservative rhythm and never assign a capacity-expansion treatment.
- **Hold:** evidence is mixed, sparse, or stable without a clear reason to change. Uncertainty favors holding.
- **Explore:** comparable history is sufficient, behavior is stable, completion is healthy, and strain, recovery debt, avoidance, and drift are low. A bounded treatment may be tried. Longer breaks or earlier long breaks are explored only for a clean return-drift pattern without blocker pressure during overtime.
- **Exploit:** apply a treatment that already won in a compatible context.

Postures guide experiment eligibility; they do not prove a treatment is beneficial.

## Replay approval

At run start, the owner may evaluate the bounded candidate catalog against at most 50 historical opportunities. Inputs, historical evidence, outcomes, and the current decision share the accepted transaction's snapshot, and device-local timezone facts are resolved natively for both current and historical opportunities.

- An outcome counts for a candidate only when the candidate's rhythm matches the rhythm actually observed.
- Comparable contexts need enough matching outcomes and acceptable guardrail burden. Multi-parameter candidates are also compared with their individual components.
- Harmful actual exposure or an antagonistic component comparison vetoes a candidate. Sparse evidence stays inconclusive and does not overturn a passing context gate. Ties keep catalog order.
- Replay never overrides recovery, fallback, or guardrail decisions. Historical evidence excludes anything recorded after its observation cutoff.

## Evidence bounds

Evidence reads are bounded in the same snapshot as the calculation: a history read admits at most 100,000 aggregate input rows, a replay read shares 1,000,000 rows across all opportunities, and each policy admits at most 4,096 experiments. Exceeding a bound fails explicitly and rolls back the attempted execution; it never substitutes an empty aggregate, truncates evidence, or deletes history. Raising a bound requires measured work and memory evidence.

Rationale: a truncated aggregate looks complete and would silently bias decisions.

## Evidence hierarchy

Analysis prefers the most comparable evidence with enough observations in both arms:

1. Exact coarse context.
2. Paired neighboring contexts with similar session, time, event, workload, energy, and environment.
3. Broader non-context evidence for the same experiment.
4. Global experiment evidence.
5. Inconclusive when none is sufficient.

Neighboring and broader evidence is discounted to a small prior. Sparse evidence never yields a confident win merely because its point estimate is positive.

## Assignment and outcomes

An eligible run receives a deterministic assignment from a persisted seed, experiment identity, policy version, participant scope, and context. Retries return the existing assignment. Control is a real assigned variant; treatment is never compared only with unrelated historical defaults.

Assignment and outcome are separate records. Outcomes (clean focus, completion, stops and focus failures, blocker pressure, skipped breaks, break overtime, same-day missed work, next-day return) attach only once mature. Unknown outcomes stay absent rather than counting as failure, and recomputing aggregates never rewrites assignments or raw outcomes.

## Conservative analysis

Binary outcomes use conservative interval comparisons with severe point-harm stops. Numeric outcomes keep counts, sums, and squared sums so noisy differences stay inconclusive until uncertainty is small.

A treatment wins only when its primary outcome improves meaningfully and every guardrail holds. Control wins when a guardrail shows conservative or severe harm. Otherwise the result is inconclusive and exploration waits for more observations.

Common guardrails: completion does not decline; stop and focus-failure rates, blocker pressure, skipped breaks, and break drift do not rise; clean focus does not fall where it is a guardrail; same-day planned work and next-day return do not worsen. An experiment may add stricter guardrails but may not omit a known material risk to reach a result sooner.

## Terminal state and cooldown

A terminal result records completed (treatment won) or abandoned (guardrails forced control) without creating a synthetic assignment. A 14-day cooldown follows. An abandoned experiment holds control during cooldown. A completed treatment applies only within its supported context and never becomes a universal preference. Lanes keep independent results; harm in a bundle does not prove each component harmful.

## Explainability

Every selected value retains policy and experiment version, previous and selected value, assigned variant, coarse context key, reason codes and posture, state scores, assignment seed identity, and later result and cooldown. User-facing explanations summarize the main reason and uncertainty in plain language without exposing raw feature vectors or claiming causation.

## Privacy and retention

Raw adaptive history stays in the vault. Derived aggregates are rebuildable. Export or synchronization of detailed Pomodoro behavior is off by default and requires an explicit audience. Sensitive traits are never inferred from Notes, Chat, Calendar titles, browsing history, or biometric data.

## Versioning

A material change to eligibility, context bucketing, assignment, outcomes, statistics, guardrails, or terminal interpretation increments the policy or experiment version, and older decisions stay interpretable under their original version. Refactors that leave semantics unchanged do not bump versions.

## Non-goals

Adaptive Pomodoro does not maximize every focus interval, diagnose burnout, attention, sleep, or mood, alter an active phase, use a cloud model to choose timer values, hide assignments or history, share productivity behavior by default, or override an explicit custom rhythm.

The current lanes are in [Adaptive experiments](adaptive-experiments.md).
