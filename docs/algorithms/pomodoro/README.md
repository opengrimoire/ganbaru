# Pomodoro algorithms

Pomodoro behavior is split into related specifications so each decision stays reviewable on its own.

- [Focus authority and evidence](focus-authority.md): what may start or advance execution, the native owner, recovery, effect fencing, and planned device ownership.
- [State machine](state-machine.md): observation priority, phase advancement, Calendar block activation, reconfiguration, and side-effect ordering.
- [Plan and history](plan-and-history.md): lazy segment persistence, future projection, inheritance, pauses, and worked examples.
- [Idle detection](idle-detection.md): platform activity sources, sampling, webcam suppression, idle backdating, and focus failure.
- [Adaptive policy](adaptive-policy.md): objectives, evidence, decision boundaries, guardrails, and privacy rules.
- [Adaptive experiments](adaptive-experiments.md): the seven bounded experiment lanes and their analysis rules.

Durable rows are summarized in [Pomodoro schema](../../data/schema/pomodoro.md). User-visible behavior lives in the [Pomodoro feature documents](../../features/pomodoro/README.md).

## Shared requirements

- Persist facts about phases that started. Derive future phases.
- Never rewrite completed or interrupted progress after a configuration change.
- Make transition decisions deterministic from explicit inputs.
- Commit canonical state before notifications, overlays, media, or window events.
- Treat desktop and mobile recovery evidence separately.
- Record adaptive decisions in the same transaction as the run or phase they select.
- Keep at most one globally active segment.

Algorithm names describe responsibilities, not file or helper names. Source may be reorganized without changing these contracts.
