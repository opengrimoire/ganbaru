# Distraction usage limits

Usage limits are independent daily or weekly budgets. They apply whenever a local adapter can observe a configured website or application, not only during Pomodoro.

## Limit model

A limit has a stable identity, an optional display name, an enabled state, at least one daily or weekly budget, and one or more linked entries. Entries can represent the same habit across platforms, such as a website host, an Android package, and a desktop application. Overlapping sources count once per limit.

Single-entry limits can derive their name from the entry; multi-entry limits need a group name. New limits default to one hour daily and no weekly budget. Each linked source uses an event-palette color, always paired with a label. Browser categories are rule presets, not limit entries; limits target explicit hosts and apps. Disabled limits stay visible but never authorize enforcement.

## Time windows

Daily budgets reset at local midnight; weekly budgets use the Monday-based local week. After a timezone change, new samples use the new local date while existing samples keep theirs, so a clock change never rewrites history.

Deleting a limit does not delete usage samples. Recreating a matching limit includes activity already recorded in the current window, so delete-and-recreate is not a hidden reset.

## Ownership

Accounting, totals, and enforcement are native and independent of WebView scheduling:

- **Desktop:** one serialized Rust owner observes selected applications, records intervals, derives exhaustion, publishes browser snapshots, and executes authorized closes. Settings read its cached projection and cannot start observations, submit intervals, or request closes.
- **Android:** the Guardian service observes and enforces selected packages, even if the app process is gone. A Rust publisher in the app process combines canonical and pending usage into shared totals and rule snapshots for Guardian; Guardian adds only later local observations on top.
- **Linked read-only devices** compute totals from accepted usage plus applicable pending usage.

Every exhausted result is bound to the active database, local window, and exact persisted limit configuration. Editing a budget or its sources, changing vaults, crossing midnight, or handoff invalidates older results. Missing, future-dated, or inconsistent results fail open. Vault handoff waits a bounded time for captured usage to reach device-local storage, or cancels.

## Counting integrity

Usage is evidence, so it must never be double-counted or invented:

- Samples carry stable identities. Retried writes, compaction, and acknowledgement keep those identities so a retry cannot count the same usage twice.
- Applying a linked snapshot and acknowledging the pending samples it includes happen in one transaction; failure retains both for retry.
- Intervals use monotonic elapsed time and split at actual local midnights, including daylight-saving changes. Long gaps and clock discontinuities produce no inferred usage.
- A full device-local spool or journal fails explicitly and stops capturing new usage rather than dropping or clamping it.

## Browser counting

Browser usage counts the active HTTP or HTTPS tab while its window is focused, including passive reading and fullscreen video. Background tabs and windows, locked browsers, extension pages, and unsupported URLs do not count. Samples contain normalized host, elapsed time, local date, and bounded timing, never full URLs, query strings, page text, playback state, or input.

## Android counting

Android counts selected packages while their activities are visible and the screen is interactive and unlocked, including passive use. Split-screen and picture-in-picture can count every visible selected package. Samples contain package identity, label, elapsed time, local date, and bounded timing, never screen content, text, taps, notifications, or unselected app history. Entries without a package identity are shown but must be reselected before Android enforcement can trust them.

## Desktop counting

Desktop counting prefers the foreground application where the operating system exposes it: Windows foreground windows, macOS frontmost applications, Linux X11 active windows, and Wayland compositor protocols that expose focus. Linux observations use application classes, process names, or compositor app IDs, never window titles.

On Wayland sessions without foreground visibility, a selected app may count while its safely matched process is open, labeled as open-app time rather than focused use. If neither is available, the source is unavailable and accumulates nothing.

## Enforcement

- **Browser:** matching top-level pages redirect to the block page, which says whether the daily or weekly limit was reached and offers no bypass.
- **Android:** an exhausted package returns the user Home and posts a rate-limited notification linking to the limit settings.
- **Desktop:** apps are closed only where a safe close adapter exists, after revalidating the exact enabled limit, linked entry, current total, app identity, protected-app policy, and observation freshness. Anything stale, removed, disabled, mismatched, or no longer exhausted performs no close.

## Presentation

Each budget shows used and available time with a stacked, labeled source breakdown; limits with both budgets show separate groups. Unavailable adapters, stale permissions, and entries that need reselection are explicit. Missing native budgets show an unavailable state, never inferred zero usage. Batching can delay the newest interval slightly, so presentation does not imply second-level precision.

## History and privacy

Usage history is local structured data. It supports totals and configuration guidance without becoming an activity diary, and retention keeps current budgets and useful trends while bounding storage.

Status: real desktop lifecycle and Android background and process-restart acceptance are pending.
