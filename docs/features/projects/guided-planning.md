# Guided planning and review

Status: Planned. The current Projects and Chat foundations (tasks, dependencies, date proposals, history, AI agents, and context packages) are the base this workflow builds on.

Guided planning helps a user turn an unclear intention into a bounded project without pretending that AI-generated plans are commitments. It works without AI through structured prompts and templates.

## Stages

The workflow separates exploration, evaluation, commitment, execution, and review. Users can stop after any stage with useful notes and no hidden tasks.

The reasoning frame is:

- **Want:** the desired outcome and personal reason.
- **Can:** available skills, time, resources, constraints, and risks.
- **Need:** requirements, dependencies, acceptance criteria, and non-negotiable obligations.

The intersection becomes a proposal, and a proposal becomes a project plan only after explicit user acceptance.

- **Exploration** captures the idea, motivation, possible outcomes, constraints, unknowns, and alternatives. It favors quick capture before demanding a detailed hierarchy.
- **Evaluation** compares value, effort, feasibility, uncertainty, opportunity cost, and current capacity. It can recommend proceeding, narrowing, postponing, researching, or declining, and it explains assumptions instead of presenting a score as objective truth.
- **Commitment** defines the outcome and acceptance criteria, scope and exclusions, milestones and tasks, owners and reviewers, dependencies, estimates, dates, required context, and review cadence. The user previews canonical changes before applying them, and partial acceptance is allowed.

The fantasy terms Genesis, Forging, Journey, and guide characters can remain an optional presentation theme. They are not canonical data states and must not obscure ordinary planning language or accessibility.

## Commitment boundary

Conversation and generated drafts are not canonical tasks. Accepted plans use typed Project commands to create or update sections, tasks, checklists, dates, dependencies, decisions, and settings. Chat messages, generated reports, and provider output are context, never substitutes for canonical task or decision state.

Generated plans prefer a small set of meaningful tasks over exhaustive decomposition. Tasks are independently trackable work; checklists are lightweight completion detail inside one task.

An AI provider never receives the complete vault by default. Context packages include only selected project material.

## Assignment and review

Human and AI assignments carry the same objective, project, authority, expected output, and review destination. An agent can propose task changes but does not commit them unless the assignment explicitly grants that typed action. Coordination rules live in [Chat agents and coordination](../chat/agents-and-coordination.md).

Review records accepted results, rejected proposals, exceptions, and follow-up work without flooding the project with low-level execution events. Detailed provider events stay in the linked run. Approval of one result does not grant standing approval for future actions.

## Requirement and decision history

Important requirements and decisions retain rationale, author, time, alternatives, and affected tasks. Later changes append history instead of overwriting why the prior decision existed. Accepted plan snapshots can support comparison, while current Project rows remain canonical.

## Sustainable capacity

Planning considers working hours, existing commitments, estimates, uncertainty, focus capacity, and recovery time. It can warn when a plan exceeds capacity and suggest scope or deadline changes. Capacity is a planning constraint, not a productivity judgment: the workflow does not manufacture urgency, shame rest, or optimize for task count.

## Replanning and reports

Date or dependency changes produce proposals for affected work. The implemented Gantt cascade is described in [Dependency date proposals](settings-and-scheduling.md#dependency-date-proposals); guided replanning adds explicit date locks and user-selected protection overrides.

Reports summarize progress, blockers, risks, schedule changes, review needs, and decisions from canonical project data. They direct attention to exceptions rather than generate status prose for its own sake.

## Repository integration and collaboration

Software projects can link an authorized working folder, Chat runs, checkpoints, commits, and review artifacts. Git remains canonical for repository content; Project tasks and decisions remain canonical organizational records.

Future collaboration applies project, task, Notes, conversation, and working-folder permissions separately. Assigning a task never implies access to every project Note or filesystem path. See [Contacts and invitations](../collaboration/README.md) and [access control](../../data/access-control.md).
