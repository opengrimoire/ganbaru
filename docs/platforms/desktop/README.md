# Desktop platforms

Linux and Windows host the complete local application, including native coding-agent execution, authorized working folders, terminals, Git, desktop media, browser native messaging, and platform-specific anti-distraction adapters. The browser native messaging host never opens the vault: it writes block events and usage to a spool in the app config directory, and the app moves them into the vault.

Shared feature behavior lives under [features](../../features/README.md). Cross-platform frontend and native composition are documented under [architecture](../../architecture/README.md). Desktop-specific documents:

- [Tray](tray.md)

## First use and device linking

The linking protocol, ownership rules, firewall authorization, and data protections are owned by [device linking and synchronization](../../data/sync.md). Desktop adds these interaction rules:

- With no active vault, setup starts with a localized welcome screen (license summary, source link, full AGPL 3.0 text, and acknowledgments) and continues to vault selection. A missing or invalid configured vault also leads to vault selection, with neutral inline recovery guidance.
- The first desktop is the administration device and runs the embedded private-LAN coordinator. After vault selection, a linking screen shows a short-lived phone pairing QR invitation. **Not now** completes onboarding without linking; closing the screen without choosing leaves onboarding unfinished.
- Data settings creates further invitations. Phones scan the QR in Ganbaru. Another desktop pastes the complete pairing code copied from the coordinator into its own Data settings, then becomes a client and stops its own coordinator. While a device enrolls, the coordinator shows its name and confirmation code in place of the QR, and Data settings opens that approval on its own.
- A linked-device control placed after Pomodoro lists devices and opens handoff actions without adding a permanent read-only warning row. Queued work uses a stable status indicator; only an explicitly running foreground action animates.
- When another device owns the vault, desktop keeps its last complete copy available read-only. Opening that copy, or a handoff that makes it read-only, shows the current main device and offers `Switch to this device` or continuing read-only.
- On Linux, the linking screen detects active UFW and firewalld configurations and offers one scoped permission action for the coordinator and LAN discovery on IPv4 and IPv6 after explaining why it is needed. The development build disables that action during onboarding because its executable cannot use the installed trusted helper. Unsupported firewall managers show a truthful manual-action state.
- On Windows, the app adds no firewall rule. The first time the coordinator listens on the local network, Windows Defender Firewall asks whether to allow the app; allowing it creates the operating system's usual rule for the whole program on the chosen network types, not a rule scoped to the linking port or subnet. The coordinator listens on the IPv4 and IPv6 wildcard addresses and starts only when linking needs it, and every connection still passes the listener's address policy, budgets, and mutual TLS. The program rule is expected to cover both listener sockets and the DNS-SD responder on UDP 5353; physical acceptance confirms it, and without discovery linked devices still follow address changes through the endpoints the coordinator reports. Users who want a narrower rule can edit it in Windows Defender Firewall.

## File drops

Desktop windows keep Tauri's native drag-and-drop handler, which takes file drops before the webview sees them: WebKitGTK reports a dropped file only as its address, so an HTML5 drop would insert that address as text or navigate the page to it. Because desktop windows host their pages as child webviews, drops arrive as webview events. Rust records the paths of the latest drop on each webview, and a feature that accepts drops listens for the drop event while its target is open and asks a command to import that drop. The command reads only paths the operating system delivered, claims a drop once, and refuses drops older than a minute, so the webview never names files to read. Quick note images accept drops this way; other HTML5 file drop targets, such as the Chat composer, do not receive desktop drops yet.
