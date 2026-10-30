# Focus authority and evidence

**Status: Partial.** A native owner on desktop and Android drives admission, transactional execution, adaptive decisions, recovery, and accepted-phase publication. Physical platform acceptance remains open. Each linked device runs its own sessions, and closed runs and device activity replicate. Live companion status is planned.

## Independent facts

A commitment is a Calendar plan. An executing run is an accepted interval with persisted state. Activity is an observation from an authorized local source or an explicit user confirmation. Freshness describes when a remote response was received. None implies the others.

A timer measures an accepted interval; it does not prove productive work. Calendar projections, phone backgrounding, screen-off time, and silence from another device are not activity evidence.

## Admission

- Desktop automatic starts require an eligible Calendar event and a new native activity observation showing input at or after the event boundary and no older than 15 seconds. Missing, malformed, future, or stale observations cannot authorize execution. While a due commitment waits for activity, the owner rechecks every 15 seconds. Manual start remains available when the source is unavailable.
- Android starts only through explicit user action. Calendar alarms carry reminder data only, never a run ID, phase plan, or running flag. Permission denial never starts a session.
- A run publishes running state only after its initial write commits. A failed write leaves no executing timer. A late start uses its actual acceptance time.
- Desktop automatic admission skips an occurrence that a fresh activity row of another linked device is focusing on. An explicit start is never filtered; the UI names the other device and its event and asks for confirmation first. Replicated rows never start, stop, or change a run on this device.
- A finished break waiting for return is not an accepted phase. Waiting for any duration cannot start focus; the user must accept the return ("Ready to return").

Rationale: without these rules, a phone alarm or a desktop that was off at event start could fabricate focus history and drive the distraction blocker or Music as if the user were working.

## Native owner

`ganbaru-pomodoro` owns admission, transitions, pauses, recovery, history, and adaptive boundary decisions. The Tauri app runs one serialized native owner per active vault (`pomodoro/native_runtime`). Svelte never runs execution timers or writes runs, segments, pauses, or adaptive rows; it sends semantic commands and paints accepted snapshots. Android lifecycle observations come from Kotlin without needing a WebView.

- Every command carries an ID. An accepted command stores an immutable receipt in the same transaction as its effects, so retries return the committed result instead of executing again. A later stopped run cannot be replaced by an earlier receipt.
- User actions are bound to the revision shown when the user clicked. Stale revisions are rejected; they never postpone execution deadlines.
- Outgoing and incoming phases, the adaptive decision, and the receipt commit in one transaction. Failed timezone resolution, evidence admission, or persistence rejects the operation without publishing execution.
- The Calendar configuration and the adaptively selected rhythm stay separate, so a policy change cannot masquerade as a Calendar reconfiguration. Accepted phase durations are fixed; only a later boundary can select another rhythm.
- Presentation subscriptions carry compact revision and availability notices, are bound to the invoking window, are limited to eight, and expire after 30 seconds without renewal. Reading presentation never admits a run.

## Recovery

Recovery reads canonical SQLite state only, never a notification projection, and repeated recovery creates no new history.

- **Desktop:** an open run found at startup closes as interrupted at its last heartbeat, bounded by the event end. Heartbeats are written every 15 seconds while a run is open.
- **Android:** a previously committed phase and its pauses resume when still valid. A phase whose deadline passed completes at that deadline and enters return wait. An elapsed event expires the run. Recorded time never exceeds the accepted phase deadline.

Rationale: desktop crashes leave no trustworthy evidence of what happened after the last heartbeat, whereas Android routinely suspends JavaScript while the committed native phase remains valid.

## Cross-feature effects

Notifications, overlays, tray, sounds, Music, the distraction blocker, and Android Guardian are effects of committed state, never inputs to it.

- Effects carry the Focus publication generation and the active vault generation, and are rechecked against current native authority immediately before delivery. Superseded, stopped, or different-run effects are dropped.
- Effect validity is a monotonic lease (15 seconds) clipped to the accepted phase and event deadlines. Return wait, failed, stopped, and expired phases cannot authorize phase-dependent rules.
- Effect delivery runs in a separate worker that coalesces to the latest request. A slow or failing platform operation never delays execution; it retries with its own backoff and reports a pending-delivery diagnostic.
- Android notifications describe only the current accepted phase. At its deadline they remind the user to return; they cannot advance to another phase. Alarm and committed-completion delivery share one receipt so a boundary alert fires once.
- On Android, the Guardian service accepts a phase only after checking current native authority through an app-private, read-only provider bound to the Rust process identity, so an old process cannot overwrite newer state.

## Vault lifecycle

Vault selection, replacement, and snapshot bootstrap freeze the native owner before closing pools, using the shared [vault operation boundary](../../data/architecture.md). Freeze immediately revokes effect admission, waits up to ten seconds for the owner and five seconds for the delivery worker, and fails quiescence on timeout instead of abandoning work. Lifecycle revisions prevent an earlier freeze or resume from overriding a later one. After resume, idle warnings must be shown and acknowledged again.

## Idle grace

Idle failure is measured from the first committed visibility acknowledgement of the current warning, using the owner's monotonic clock. Missing visibility or a failed acknowledgement write cannot consume the user's grace period. See [Idle detection](idle-detection.md).

## Linked devices

Each linked device runs its own native owner and writes only its own runs, so Focus works the same on every linked device. A run replicates once closed, and the device activity row is rewritten in the same transaction as each start, phase change, pause, resume, and stop, never on heartbeat. A running row expires 10 minutes after its planned phase end and a paused row 30 minutes after the pause began, so a device that goes silent stops blocking admission elsewhere. Expiry is compared with the reading device's clock, so clock skew between devices shortens or lengthens that window. Activity is advisory: it never counts as activity evidence for this device. There is no controller ownership and no remote control: a device never starts, stops, or retargets another device's run, and replication never causes native execution.

## Planned: companion freshness

Live status uses a separate connection from durable replication. During an active companion connection, heartbeats run every 15 seconds and status expires after 45 seconds without a fresh response, measured with local monotonic time and connection generations rather than peer clocks.

When status expires, show the last confirmed state and "Status unavailable." Do not infer focus, idle, failure, or completed phases. Cached deadlines may produce clearly labeled scheduled reminders only. Phone activity does not reset the desktop idle clock or override webcam suppression and source failures. Phase-dependent Music and the distraction blocker consume confirmed state with bounded validity, and Android background restrictions appear as degraded connectivity.
