# Chat execution and workspace

Status: Implemented on desktop. Mobile omits local coding execution.

## Execution targets

Every organizational run has exactly one execution target: an authorized project working folder, a private scratch generation owned by the initiating reply thread, or no target for conversation-only work. External folder bindings are device-local and must resolve through the project's authorization; provider-native working-directory state never authorizes a folder. Target resolution and scratch rules live in [Chat access control](../../data/access-control.md#execution-targets).

Scratch output stays private until the user explicitly promotes it into a project folder. Chat settings rank the scratch generations that are safe to reclaim on this device by size, without deleting anything, and cleanup stays previewed and confirmed.

## Runs and provider threads

A run links one organizational request to one provider session and its canonical events. A conversation can contain many runs, and a run can hand off to a new provider session when continuation is unavailable or inappropriate. Hidden session handoffs preserve the human conversation without pretending different providers share native thread state.

A later assignment of the same agent in the same reply thread and execution target continues the earlier native session only when its continuation key is unchanged: the authorization scope, the provider instance, the model selection, and the system-level instructions (the agent framing with its name and role, the context package format, and the agent's own instructions). A changed key or an authorization that is no longer allowed starts a new session with the full context package, and a retry of the same assignment always starts a new session, because a failed run may have left native state half applied. Each run records why it started (fresh, continuation, handoff, or retry) and the digests of its key. A continuation whose provider cannot find the earlier native session fails only that run, and the assignment hands off at once to a new session with the full context package. A provider that silently opens a new session instead receives the full package in place of the delta, and the run records that handoff. Approvals and questions the lost session left open close as interrupted. If the application stops before the handoff run starts, the assignment retries in a new session after restart.

Cancellation targets the active run and its owned process tree. It never deletes prior events or cancels unrelated parallel work. Stop settles the turn as interrupted within a few seconds even when the provider does not confirm it, and the provider session restarts for the next turn. Chat settings list the provider and terminal processes Chat started and can stop one after a confirmation and an identity check ([Chat security](../../data/security/chat.md)).

On Linux, provider sessions and local provider servers run inside a user systemd resource scope with low CPU and IO weight, a task ceiling, and memory ceilings that leave the desktop headroom, so a runaway provider slows or stops itself instead of the desktop. When the scope stops a provider for memory, the conversation shows a notice before the session failure. Where user systemd is unavailable, such as inside Flatpak, or a scoped launch fails, providers start directly for the rest of the app run, because the scope must never block a provider. Chat terminals and short provider probes are never scoped.

A run executes on one device, and its process, interactive requests, terminal, and execution state stay there. Other linked devices see its messages and can complete or cancel its assignment, which stops the run on the executing device. A phone with the reply thread open reads the runs of the thread's recent work from the executing desktop over the LAN, read-only, as the [exchange](../../data/sync.md#exchange) describes: a working run streams as it happens, and a settled run shows its whole turn in place of the replies projected from it, as on the desktop, while a failure reply keeps its Retry and an approval or question reply stays while the work is active. A settled run the phone read earlier in the session shows again at once when its thread reopens, and the phone confirms it with the desktop. Otherwise, off the LAN, the phone shows the replies alone. The execution timeline itself never replicates.

## Interactive requests and approvals

Questions, permission requests, plan confirmations, and provider-native choices appear as validated interactive requests with a stable identity and exactly one terminal resolution. A provider may hold several requests open at once; each shows its own panel, oldest first, and is answered by its own identity. The UI states the requested operation and effective authority: approval panels show the concrete subject (the exact command, path, URL, pattern, query, or tool) outside the collapsed details, and "Allow for session" lists every permission it grants. Answering a provider request never bypasses Ganbaru's own access checks, and stale or duplicate responses are rejected. Interactive requests stay on the device that executes the assignment. A linked phone on the LAN can answer an approval there with Allow once, Deny, or Cancel turn from the reply thread, and the executing desktop resolves it through the same path as its own panel; Allow is offered only with the whole concrete subject shown, and session grants, edited tool input, and questions are answered on the desktop.

## Workspace panels

The secondary panel and the bottom panel belong to the selected channel, not to a thread: each channel keeps its own open tabs, selected tab, terminals, and terminal layout, so two channels in the same folder stay separate and switching channels and back restores them. A reply thread shown in its own workspace tab keeps panels, terminals, and open state apart from its channel, while folders and authority still come from the channel; its terminal layout lasts for the app session only. Each channel also owns whether each panel is open: a channel starts with both closed, the header toggles and shortcuts act on the selected channel, and switching tabs inside a panel never closes it. Open state lasts for the app session only; panel sizes stay shared across channels so the conversation column does not jump when switching. Files, Source control, and Terminal target one working folder, which defaults to the channel's primary folder. Source control can switch the panel to another folder of the project bound on this device, and new terminals start in the chosen folder. The chat's worktree environment applies only while the panel targets the chat's own folder. Review and the preview browser still follow the selected thread or assignment, because their diffs and grants are per session.

## Files and editor

The Files surface is a bounded view of the authorized execution target. Reads, searches, edits, and saves validate normalized relative paths and reject escapes, unsupported files, oversized content, and stale revisions. Saving against a changed file requires reconciliation instead of overwriting newer content. Syntax highlighting and previews never expand the write boundary.

## Review and source control

Review presents file changes, diffs, checkpoints, and source-control status, and commits staged changes with a message that may span several lines. The filesystem and Git remain canonical for workspace content; the vault stores runs, checkpoint metadata, and review links. Runs can checkpoint before and after risky work, and restore is an explicit authorized action that reports uncommitted or conflicting state before mutating anything. Restores are confirmed in a dialog that shows staged changes the restore would affect and whether the provider can roll back its own state.

When a checkpoint pair is unavailable, review falls back to the provider-reported turn and then to the working tree. Only a typed not-found reason selects a fallback; permission and storage errors stay errors.

The commit and change request forms can ask the selected thread's agent, when it is idle in the same working folder, to draft the text from bounded Git context (changed paths, a size-capped patch, and recent subjects), stating every cut in the prompt. Drafting happens only on request, as a visible turn in that thread. The draft fills the field only after the turn completes and only when the user has not edited the field since asking; otherwise it is offered, and an empty or oversized draft is refused instead of cut.

Review can cover a whole assignment across its runs, which is the default for work ready for review. Files the assignment did not author, such as files brought in by a merge or pull, are flagged and hidden until shown. The assignment diff reads the working folder on the executing device; on another device review names that device and lists the changed files instead.

A changed file too large to review is listed with a notice instead of a diff, and it can still be staged, unstaged, or discarded as a whole.

A working folder may be a subdirectory of its repository. Review and source control then list paths from the repository root, and staging, unstaging, discarding, and diffs act on exactly the listed file, as the user's own `git` would from that folder.

## Terminal

Terminals are bound to one channel and one authorized working folder, and start in that folder rather than in a thread's worktree. The frontend receives a narrow stream and input boundary, not a generic process API. Closing a terminal view does not necessarily stop its process, and lifecycle and stop controls stay truthful. Output is bounded, untrusted text; escape sequences cannot become application commands or links without validation. Terminals start without the app's inherited multiplexer and development server port variables, replaying a terminal view resets terminal modes such as the alternate screen and hidden cursor, and terminal history is never persisted.

Terminals start without a confirmation, like a terminal opened in any editor: the user's own shell in a folder they bound is not a privilege escalation over their account.

## Browser preview

Previews use isolated app-owned webviews with narrowly scoped capture controls. Each working folder keeps its own preview logins, site storage, and cache on the device, separate from the app and from other folders; they are deleted when the folder is unbound and can be cleared from Chat permission settings. A preview cannot browse local files, inherit app capabilities, or become general desktop capture. Screenshots and recordings become managed artifacts under `assets/chat/browser-artifacts/` only through an authorized capture action.

The model can use loopback previews such as a local development server. A thread remembers the local addresses the user opened; when a page or the model moves to a different local port, the Browser panel asks once with an Allow button and the thread remembers the answer. If the run's sandbox has no network access, the Browser panel also asks once per session before the model may use preview tools. To let it work on an external site, for example one the user is logged into, the user allows that site once from the Browser panel; the choice is remembered for that site in that thread and can be revoked from the panel or from Chat permission settings.

When a previewed page asks for the camera, microphone, location, notifications, clipboard, or another capability, the Browser panel names the site and asks; the answer is remembered for that site in that thread. Page dialogs (`alert`, `confirm`, `prompt`, and leave-page confirmations) appear in the Browser panel too. Dialogs a page opens while the model is clicking, typing, or scrolling are dismissed and reported to the model.

## Workspace observation

Filesystem observation keeps Files, Review, and source-control views fresh. Events are debounced, bounded, and reconciled with authoritative reads; observation never replaces revision checks.

## Internal host tools

Runs receive an ephemeral loopback MCP endpoint for narrowly scoped application-owned tools, such as bounded channel reads and workspace operations. It exists only for the authorized run, re-checks authorization on every call, records invocations, and is never a permanent service, participant, or agent. See [Chat access control](../../data/access-control.md#internal-host-tools).

See [AI provider runtimes](../ai/provider-runtimes.md).
