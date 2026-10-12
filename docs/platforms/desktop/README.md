# Desktop platforms

Linux and Windows host the complete local application, including native coding-agent execution, authorized working folders, terminals, Git, desktop media, browser native messaging, and platform-specific anti-distraction adapters.

Shared feature behavior lives under [features](../../features/README.md). Cross-platform frontend and native composition are documented under [architecture](../../architecture/README.md). Desktop-specific documents:

- [Tray](tray.md)

## First use and device linking

The linking protocol, ownership rules, firewall authorization, and data protections are owned by [device linking and synchronization](../../data/sync.md). Desktop adds these interaction rules:

- With no active vault, setup starts with a localized welcome screen (license summary, source link, full AGPL 3.0 text, and acknowledgments) and continues to vault selection. A missing or invalid configured vault also leads to vault selection, with neutral inline recovery guidance.
- The first desktop is the administration device and runs the embedded private-LAN coordinator. After vault selection, a linking screen shows a short-lived phone pairing QR invitation. **Not now** completes onboarding without linking; closing the screen without choosing leaves onboarding unfinished.
- Data settings creates further invitations. Phones scan the QR in Ganbaru AI. Another desktop pastes the complete pairing code copied from the coordinator into its own Data settings, then becomes a client and stops its own coordinator.
- A linked-device control placed after Pomodoro lists devices and opens handoff actions without adding a permanent read-only warning row. Queued work uses a stable status indicator; only an explicitly running foreground action animates.
- When another device owns the vault, desktop keeps its last complete copy available read-only. Opening that copy, or a handoff that makes it read-only, shows the current main device and offers `Switch to this device` or continuing read-only.
- On Linux, the linking screen detects active UFW and firewalld configurations and offers a scoped permission action after explaining why it is needed. The development build disables that action during onboarding because its executable cannot use the installed trusted helper. Unsupported firewall managers show a truthful manual-action state.
