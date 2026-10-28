# Chat execution and workspace

Status: Implemented on desktop. Mobile omits local coding execution.

## Execution targets

Every organizational run has exactly one execution target: an authorized project working folder, a private scratch generation owned by the initiating reply thread, or no target for conversation-only work. External folder bindings are device-local and must resolve through the project's authorization; provider-native working-directory state never authorizes a folder. Target resolution and scratch rules live in [Chat access control](../../data/access-control.md#execution-targets).

Scratch output stays private until the user explicitly promotes it into a project folder, and scratch cleanup is previewed and confirmed.

## Runs and provider threads

A run links one organizational request to one provider session and its canonical events. A conversation can contain many runs, and a run can hand off to a new provider session when continuation is unavailable or inappropriate. Hidden session handoffs preserve the human conversation without pretending different providers share native thread state.

Cancellation targets the active run and its owned process tree. It never deletes prior events or cancels unrelated parallel work.

A run executes on one device, and its process, approvals, questions, terminal, and execution state stay there. Other linked devices see its messages and can complete or cancel its assignment, which stops the run on the executing device.

## Interactive requests and approvals

Questions, permission requests, plan confirmations, and provider-native choices appear as validated interactive requests with a stable identity and exactly one terminal resolution. The UI states the requested operation and effective authority: approval panels show the concrete subject (the exact command, path, URL, pattern, query, or tool) outside the collapsed details, and "Allow for session" lists every permission it grants. Answering a provider request never bypasses Ganbaru's own access checks, and stale or duplicate responses are rejected. Execution and its interactive requests stay on the device that executes the assignment: other devices see its progress but cannot answer its approvals or questions.

## Files and editor

The Files surface is a bounded view of the authorized execution target. Reads, searches, edits, and saves validate normalized relative paths and reject escapes, unsupported files, oversized content, and stale revisions. Saving against a changed file requires reconciliation instead of overwriting newer content. Syntax highlighting and previews never expand the write boundary.

## Review and source control

Review presents file changes, diffs, checkpoints, and source-control status. The filesystem and Git remain canonical for workspace content; the vault stores runs, checkpoint metadata, and review links. Runs can checkpoint before and after risky work, and restore is an explicit authorized action that reports uncommitted or conflicting state before mutating anything.

When a checkpoint pair is unavailable, review falls back to the provider-reported turn and then to the working tree. Only a typed not-found reason selects a fallback; permission and storage errors stay errors.

A changed file too large to review is listed with a notice instead of a diff, and it can still be staged, unstaged, or discarded as a whole.

A working folder may be a subdirectory of its repository. Review and source control then list paths from the repository root, and staging, unstaging, discarding, and diffs act on exactly the listed file, as the user's own `git` would from that folder.

## Terminal

Terminals are bound to one authorized execution target and thread. The frontend receives a narrow stream and input boundary, not a generic process API. Closing a terminal view does not necessarily stop its process, and lifecycle and stop controls stay truthful. Output is bounded, untrusted text; escape sequences cannot become application commands or links without validation.

The first terminal for a working folder and execution environment in each app launch asks for a native confirmation that names the folder and shell. Because opening the Terminal panel starts a terminal, this usually appears the first time the panel opens for a folder after Ganbaru starts. Later terminals in that folder start without asking until the app restarts or the folder is unbound, archived, or removed.

## Browser preview

Previews use isolated app-owned webviews with narrowly scoped capture controls. Each working folder keeps its own preview logins, site storage, and cache on the device, separate from the app and from other folders; they are deleted when the folder is unbound and can be cleared from Chat permission settings. A preview cannot browse local files, inherit app capabilities, or become general desktop capture. Screenshots and recordings become managed artifacts under `assets/chat/browser-artifacts/` only through an authorized capture action.

The model can use loopback previews such as a local development server. A thread remembers the local addresses the user opened; when a page or the model moves to a different local port, the Browser panel asks once with an Allow button and the thread remembers the answer. If the run's sandbox has no network access, the Browser panel also asks once per session before the model may use preview tools. To let it work on an external site, for example one the user is logged into, the user allows that site once from the Browser panel; the choice is remembered for that site in that thread and can be revoked from the panel or from Chat permission settings.

When a previewed page asks for the camera, microphone, location, notifications, clipboard, or another capability, the Browser panel names the site and asks; the answer is remembered for that site in that thread. Page dialogs (`alert`, `confirm`, `prompt`, and leave-page confirmations) appear in the Browser panel too. Dialogs a page opens while the model is clicking, typing, or scrolling are dismissed and reported to the model.

## Workspace observation

Filesystem observation keeps Files, Review, and source-control views fresh. Events are debounced, bounded, and reconciled with authoritative reads; observation never replaces revision checks.

## Internal host tools

Runs receive an ephemeral loopback MCP endpoint for narrowly scoped application-owned tools, such as bounded channel reads and workspace operations. It exists only for the authorized run, re-checks authorization on every call, records invocations, and is never a permanent service, participant, or agent. See [Chat access control](../../data/access-control.md#internal-host-tools).

See [AI provider runtimes](../ai/provider-runtimes.md).
