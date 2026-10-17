# Distraction blocker runtime acceptance

**Status: Partial.** Automated native accounting and enforcement coverage exists; the physical platform cases below are pending. Unit tests and source inspection do not establish operating-system or WebView lifecycle behavior. Rules and limits are specified in [Distraction blocker](../features/distractions/README.md).

The core requirement on both platforms is that native code owns usage accounting and enforcement, so both keep working while the WebView is suspended, and stale context (an old rule, vault, phase, or day) never authorizes a block.

## Desktop

Use a disposable selected application and a separate protected application. Record the operating system, display server or compositor, rule, and result.

- Suspend the WebView while a selected application stays open. Daily and weekly usage keep counting, an exhausted limit still closes the application, and the UI catches up on return.
- Disable a rule, switch vaults, or complete or abort a handoff while an interval is pending. Old observations never close an application or republish an obsolete exhaustion.
- Suspend the system past the supported evidence gap. The gap is not counted as usage.
- Verify foreground counting on X11, Windows, and supported compositors. Where Wayland does not expose foreground identity, selected-process counting and its settings label are used; unsupported observation stays unavailable without invented usage.
- Replace a selected process during a close attempt. The captured process identity does not authorize closing its replacement, and protected aliases are never closed.
- Exit and restart with pending evidence. The device-local spool survives and drains; stale browser and phase snapshots do not authorize enforcement.
- Start with no vault or a deleted development vault. Observation waits for a valid selection, and idle observation does not create a spool.
- Run a linked read-only device offline, reconnect, and repeat an exchange after an uncertain response. Totals stay consistent without duplicate or lost usage.
- With the WebView suspended, Pomodoro-dependent blocking follows native Focus phase entry, pause, resume, completion, and expiry. A delayed phase publication after a run or vault change does not authorize enforcement.

## Android

Also run the distraction blocker cases in [Android acceptance](android.md#distraction-blocker).

- Suspend the WebView with the process and Guardian available. Shared usage refreshes and exhausted limits enforce.
- Usage recorded between native capture and rule application is counted exactly once.
- Disable or replace a rule, switch vaults, or complete or abort a handoff during publication. An old capture never reinstalls the earlier rule snapshot.
- Delay Guardian IPC during configuration changes, vault selection, and exit. Busy changes return a retry error, and shutdown does not wait on the publisher.
- Remove the application process while Guardian remains. Local counting and retained enforcement continue.
- Interrupt linked read-only acknowledgement after the accepted cache commits. Totals stay consistent across retries and restart.
- Cross local midnight during a capture and change the timezone. Old-window totals never authorize a new-window block.
- Verify fresh journal creation, rejection of unsupported development journal schemas (which require an explicit app-data reset), checkpoint rollback, and cross-process acknowledgement on a real runtime. JVM and Rust tests do not execute Android's SQLite or ContentProvider lifecycle.
