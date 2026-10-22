# Anti-distraction browser integration

The Chromium extension is a small enforcement and status client for the local Ganbaru application. Firefox should later implement the same product semantics through its own adapter.

## Responsibilities

The extension:

- Connects to the local native messaging host.
- Receives rule, phase, and limit state from the host on demand.
- Counts active, focused website time and reports it by host only.
- Redirects blocked top-level navigation to an extension-owned block page.
- Rechecks open tabs when rules, phase, or budget state changes.
- Reports bounded diagnostics and normalized block events.

It does not reproduce the settings editor, inspect all traffic, inject UI into arbitrary pages, or contact a remote Ganbaru service. It requests no host access: tab URLs come from the `tabs` permission and navigation events from `webNavigation`, so a compromised extension still cannot read or change page content.

## Native messaging

Messages are typed JSON with a 1 MiB request bound, and the host validates every field before using it. An unknown message type returns an error reason without applying state. There is no protocol version: the extension and host ship from the same source and change together. The extension treats an error or missing response as not blocked, so a broken connection fails visibly (disconnected status) and open. The wire schema lives in source (`crates/ganbaru-native-messaging/`), not in this spec. Block events are recorded only in the vault database when this device may write it; the host keeps no device-local log of blocked hosts.

No installer registers the native host yet. A developer script (`apps/client/scripts/browser-extension/install-native-host.mjs`) writes the host manifest and launcher on Linux and macOS for an unpacked extension; Windows needs the manifest and registry key by hand. Browser blocking must not be described as automatically installed on any platform until signed packaging owns setup and uninstall.

## Connection and stale state

Phase snapshots expire at their accepted validity deadline (or when their published remaining time elapses) even if their heartbeat is fresh. Budget snapshots come from native accounting, and the host rejects an exhausted result whose limit configuration changed, whose local day ended, whose timestamp is in the future, or whose arithmetic is inconsistent. Phase and budget rules each follow their own freshness contract.

If the host is unavailable, the extension shows a disconnected state and stops enforcing at once; if the app stops publishing state, the host stops enforcing once the snapshot expires. It does not guess whether the browser, registration, app, or vault caused the failure. Browser start triggers a fresh state request, the extension polls every 30 seconds so app start and phase changes arrive without a browser restart, and `Recheck now` flushes usage, refreshes state, and reevaluates tabs.

## Block page

The block page is deliberately quiet: the normalized host, the rule or exhausted budget, remaining focus time when relevant, and a close-tab action. It has no feeds, rewards, streaks, statistics, browsing history, or quick bypass; access changes happen in Ganbaru settings where their scope is explicit. It uses only extension-owned assets.

## Popup

The popup shows connection, active phase, remaining time, the most recent blocked host, and recheck or open-app actions. It is a status and recovery surface, not a second settings app.

## Safety allowlist

The extension evaluates only `http` and `https` pages, so its own pages, browser settings and extension management, new-tab pages, and other browser-internal pages are never blocked. The host also never blocks or limits loopback hosts (`localhost`, `*.localhost`, `127.0.0.1`, `::1`) so local setup pages stay reachable, and reports `browser safety allowlist` as the matched rule when it wins. Provider authentication pages are not exempt: a user rule that matches them applies.

## Privacy

The host receives only the origin and path of a page (query strings, fragments, and credentials are removed in the extension) and stores only normalized hosts. The extension keeps the original URL of a blocked tab in browser session storage, which is never written to disk, so it can restore the tab when the block ends. Page content is not collected for basic blocking. A future content script would run only on documented domains and return the minimum local metadata a user-enabled rule needs. Private browsing stays off unless the user enables the extension there, and multiple browser profiles connect as separate clients.

## Planned tab actions

Work environments may open or focus configured task resources and optionally pin or group them, preferring to reuse an existing matching tab. Closing or replacing user tabs is off by default and requires an explicit environment policy.
