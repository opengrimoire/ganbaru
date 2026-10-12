# Work environments

Work environments are a planned context-template system for preparing the user's workspace around a Calendar block or manual activity. They are distinct from AI execution environments and do not grant filesystem or Chat authority.

## Portable context

A work environment can eventually define portable intent such as:

- Project and optional task, Note, or Chat destination.
- Music phase assignments.
- Distraction rule overrides.
- Edge-panel context.
- Preferred application and browser resources expressed as user-approved references.

Portable intent can sync in the future even when a device cannot perform every action.

## Desktop actions

Desktop adapters may offer:

- Open an explicitly selected application.
- Open or focus approved browser task resources.
- Minimize a previously activated environment window when safely identified.
- Prepare Music and the relevant Ganbaru AI context.

Closing an application is never a generic environment action. If supported, it uses the same on-demand selection, protected-app policy, exact process identity, fresh persisted authorization, confirmation, and fail-safe behavior as [anti-distraction native enforcement](distractions/native-apps.md). Process name or PID alone is insufficient.

The app does not close terminals, editors, browsers with unsaved work, system utilities, shells, generic runtimes, or unrelated processes merely because they are absent from the new template.

## Activation

An environment can be assigned to a Calendar event or activated manually. Activation produces a preview of supported actions on the current device. Destructive or state-discarding actions require explicit confirmation.

Context activation preserves drafts, open review state, active timers, and active provider runs. It can suggest or open a linked conversation but never retargets provider continuation, changes an execution folder, or broadens participant access.

Unsupported actions remain visible as unavailable only when that helps explain portability; otherwise they are omitted from the normal action flow.

## Templates and overrides

Events reference a stable environment template. A future activation reads the then-current template unless the event stores an explicit snapshot for a behavior that must remain stable.

Per-event overrides are typed by owning feature, such as a Music phase override. They do not become one unvalidated environment JSON blob.

Precedence between global defaults, project defaults, environment intent, event snapshots, and event overrides must be documented per feature before activation ships.

## Mobile boundary

Mobile can consume portable context and feature-level defaults but cannot orchestrate arbitrary applications like a desktop. Android selected-app distraction blocking is a separate safety feature, not environment window management.

## AI execution distinction

A human work environment prepares the person's context. An AI execution environment is exactly one authorized project working folder or private scratch generation resolved for a run. Activating a work environment cannot grant an agent access, choose an unapproved folder, or change a running provider session.

See [Calendar](calendar/README.md), [Music automation](music/automation.md), [Distraction blocker](distractions/README.md), [Edge panel](edge-panel.md), and [Chat execution](chat/execution-and-workspace.md).
