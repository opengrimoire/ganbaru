# Anti-distraction browser integration

The Chromium extension is a small enforcement and status client for the local Ganbaru AI application. Firefox should later implement the same product semantics through its own adapter.

## Responsibilities

The extension:

- Connects to the local native messaging host.
- Receives versioned rule and limit state.
- Counts active, focused website time with host-only samples.
- Redirects blocked top-level navigation to an extension-owned block page.
- Rechecks open tabs when rules, phase, or budget state changes.
- Reports bounded diagnostics and normalized block events.

It does not reproduce the settings editor, inspect all traffic, inject UI into arbitrary pages, or contact a remote Ganbaru service.

## Native messaging

Messages are versioned, bounded, and validated on both sides. Unknown or malformed messages are rejected without applying state, and an unsupported protocol version fails visibly and open. The wire schema lives in source (`crates/ganbaru-native-messaging/`), not in this spec.

Native-host registration is automated on Linux and macOS. Windows manifest and registry registration remain a manual packaging gap; Windows browser blocking must not be described as automatically installed until a signed installer owns setup and uninstall.

## Connection and stale state

Phase snapshots expire at their accepted validity deadline (or when their published remaining time elapses) even if their heartbeat is fresh. Budget snapshots come from native accounting, and the host rejects an exhausted result whose limit configuration changed, whose local day ended, whose timestamp is in the future, or whose arithmetic is inconsistent. Phase and budget rules each follow their own freshness contract.

If the host is unavailable, the extension shows a disconnected state and stops enforcing once the snapshot expires. It does not guess whether the browser, registration, app, or vault caused the failure. Browser start and app start each trigger a fresh state request, and `Recheck now` flushes usage, refreshes state, and reevaluates tabs.

## Block page

The block page is deliberately quiet: the normalized host, the rule or exhausted budget, remaining focus time when relevant, and a close-tab action. It has no feeds, rewards, streaks, statistics, browsing history, or quick bypass; access changes happen in Ganbaru AI settings where their scope is explicit. It uses only extension-owned assets.

## Popup

The popup shows connection, active phase, remaining time, the most recent blocked host, and recheck or open-app actions. It is a status and recovery surface, not a second settings app.

## Safety allowlist

The extension never blocks its own pages, browser settings and extension management, new-tab pages, local setup pages, required provider authentication, or other browser safety surfaces. The explanation names this allowlist when it wins.

## Privacy

Full URLs, query strings, and fragments are not stored. Page content is not collected for basic blocking. A future content script would run only on documented domains and return the minimum local metadata a user-enabled rule needs. Private browsing stays off unless the user enables the extension there, and multiple browser profiles connect as separate clients.

## Planned tab actions

Work environments may open or focus configured task resources and optionally pin or group them, preferring to reuse an existing matching tab. Closing or replacing user tabs is off by default and requires an explicit environment policy.
