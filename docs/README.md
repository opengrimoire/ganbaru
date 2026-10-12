# Documentation

Ganbaru AI documentation describes what the app is supposed to be and why its important decisions were made. It is organized by the kind of decision it records. Start here instead of opening the largest specification directly.

## Start here

| Area | Read this first | Purpose |
| --- | --- | --- |
| Product | [Product direction](product/README.md) | Product identity, principles, scope, and system ownership |
| Features | [Feature index](features/README.md) | Current capability status and user-facing feature contracts |
| Roadmap | [Roadmap](ROADMAP.md) | Remaining work, dependencies, and deferred areas |
| Architecture | [Architecture](architecture/README.md) | Current system boundaries, technology choices, and platform composition |
| Platforms | [Platforms](platforms/README.md) | Platform capabilities, permissions, native services, and release validation |
| Data | [Data index](data/README.md) | Sources of truth, schema domains, invariants, hazards, security, and sync |
| Algorithms | [Algorithm index](algorithms/README.md) | Stable pure-logic rules and worked examples |
| Interoperability | [Interoperability](interop/README.md) | Standards scope, preservation, conformance, and client behavior |
| Performance | [Performance](performance/README.md) | Measurement rules, benchmark harness, and recorded results |
| Testing | [Testing](testing/README.md) | Validation gates, test design, and resource constraints |
| Project operations | [Operations](operations/README.md) | Repository rules, releases, and release notes |

## Status language

Feature documentation can describe both shipped behavior and the intended end state. Use these labels so readers do not have to infer which is which:

- **Implemented:** present in the current repository and available on the stated platform.
- **Partial:** a useful slice exists, but the documented feature still has material gaps.
- **Planned:** accepted product direction with no complete user-facing implementation.
- **Deferred:** intentionally outside the active delivery sequence.
- **Reference:** a normative rule, decision, standard, or historical measurement. It is not a feature-status claim.

A feature's main document states its current status in one sentence near the beginning. Target behavior belongs in clearly named sections such as `Target behavior`, `Planned work`, or `Open decisions`.

## Where information belongs

- `product/` explains what Ganbaru AI is, why it exists, and which system owns each cross-feature responsibility.
- `features/` specifies user-facing behavior. Large domains have one `README.md` and a small set of focused supporting documents.
- `architecture/` describes code boundaries and durable technology decisions. Exact dependency versions remain authoritative in manifests.
- `data/` describes sources of truth, durable storage, authorization, and cross-domain invariants. Exact SQL remains authoritative in migrations.
- `algorithms/` contains logic that should remain understandable independently of UI and persistence code.
- `interop/` records external standards, preservation rules, fixtures, and client observations.
- `platforms/` records operating-system capabilities, lifecycle, permissions, native services, and acceptance requirements.
- `performance/` separates methodology from immutable recorded measurements.
- `testing/` defines validation gates, infrastructure, test design, and manual acceptance matrices.
- `operations/` contains contributor and maintainer procedures.
- `development/` contains small contributor-facing implementation references.

## Maintenance rules

Docs record intent, not a transcript of development. Write down the intended behavior, the boundaries and invariants that future changes must preserve, and the rationale for decisions that would otherwise prompt "why do we do it this way?". Source code, manifests, migrations, and generated platform projects own exact implementation shape.

Do not add to the docs for every change. Update a document only when the change alters intended behavior, a user-facing guarantee, a source of truth, a security or authorization boundary, a status claim, or an important decision. A bug fix, refactor, rename, visual tweak, or performance tuning usually needs no doc change.

Keep out of the docs:

- Micro-interaction detail such as keystroke edge cases, hover or animation behavior, and pixel values, unless it is a deliberate product contract.
- Implementation tours of private modules, helper names, or step-by-step code flow.
- Mid-development notes, changelog-style history, and "X was removed or replaced" narration. Describe the current intended state. Keep a short "why not X" only when it explains an important decision.
- Repeated "pending acceptance" notes. State acceptance gaps once in the owning feature, platform, or testing document.
- Compatibility or migration narration for internal formats. There are no external users yet, so only the current format is documented.

Avoid repeating the same contract in multiple places. Choose one normative document and link to it:

- Feature docs own user behavior.
- Data docs own persistence and authorization rules.
- Algorithm docs own pure decision rules.
- The roadmap owns delivery order, not feature design.

Split a document when it combines independent audiences or workflows, not only because it is long. A conformance checklist or decision log can remain one file when splitting would make it harder to use.

When a status claim changes, update the feature index and the domain's main document. Update the roadmap only if the delivery horizon changed. Recorded benchmark rows and dated architecture decisions are historical records and must not be rewritten to resemble the present.
