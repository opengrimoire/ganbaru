# Local vault handoff acceptance

**Status: Partial.** Automated ownership, transport, and reconciliation tests pass; physical desktop and Android acceptance is pending. Record the result below when a maintainer completes it. The handoff model is specified in [Sync](../data/sync.md).

## Required setup

- One supported Linux or Windows desktop and one Android 10 or newer phone on the same private LAN.
- Builds from the same revision, with a fresh Android installation available for the first pass.
- A desktop vault containing an identifiable Calendar event with an alarm and Pomodoro configuration, a Project task and managed project file, a Note and attachment, a Chat channel, a Quick note, a custom theme, a distraction usage limit and history, a Music item and playlist, and a profile or project asset. Keep one music file and one project folder outside the vault.

## Acceptance script

1. **Pairing onboarding.** After choosing a desktop vault, the linking screen opens with a QR code and renewal countdown, and the code renews automatically. `Not now` loads the app without completing onboarding, so the linking screen returns after restart. On Android, scan from a normal handheld distance; the camera stops, the success state names the desktop, and Continue opens the read-only copy before the main-device choice. An untouched starter vault shows no replacement warning. Data settings and the top-bar control reopen the same scanner.
2. **First ownership with local data.** On a clean run, skip linking, add Android-only data, then link and choose `Use on this device`. A warning explains that the data cannot be merged and that a backup goes to Downloads. Cancel starts nothing. Confirming makes desktop read-only before Android becomes writable, every prepared record and asset opens on Android, and the backup recovers the Android-only data. External music and project bytes are not copied.
3. **Offline owner.** Restart both apps while Android owns the vault. With desktop unreachable, edit on Android; scheduled Calendar alarms and anti-distraction enforcement continue. Record distraction usage on the disconnected desktop.
4. **Refresh.** Reconnect and refresh desktop. Both devices show the combined distraction usage total exactly once. A Calendar alarm change reaches Android's native schedule only after Android refreshes.
5. **Transfer back.** Choose `Use on this device` on desktop. The Android edit arrives, roles swap, and restart preserves them. With the owner closed for over five seconds, a switch attempt stops promptly with a main-device-unavailable message and changes nothing; an explicit retry succeeds once the owner returns.
6. **Blocked and interrupted transfers.** Active Chat work or an active Pomodoro session blocks transfer with a clear message. Interrupt connectivity before and after commit in separate passes; retry preserves the last committed owner and never creates a second writer.
7. **Unlink and recovery.** Unlink never promotes a read-only copy. A removed offline device learns on its next request that its preserved copy stays read-only and cannot refresh or transfer. The coordinator refuses to remove the owning device. The separately confirmed recovery action on a non-owner creates an explicitly separate writable copy.
8. **Version mismatch.** Builds with different embedded migration sets cannot pair, and already linked mismatched devices reject refresh and ownership before any snapshot or replacement. Updating both restores transfer.
9. **Desktop with local data.** Requesting first ownership on a desktop that already holds independent data shows the replacement warning; confirmation preserves the old vault in a visible sibling folder.

## Selection and restore cases

- Switching desktop vaults, including with an idle or break overlay visible, leaves no old overlay, tray action, or delayed playback callback able to control the new vault.
- A failed drain of native delivery keeps the active folder and reports the failure; a concurrent folder action reports busy.
- A blocked Music source during vault selection retains the track as an interruption and never plays from an old load.
- On Android, canceling the backup picker preserves the active vault. Restoring revokes old playback and notification surfaces before replacement.
- A corrupt or incomplete archive is rejected before the active vault changes, and a later valid restore proceeds.

## Automated durability coverage

Ownership tests inject failures before file replacement and during directory sync after replacement, for registration, outgoing commit, and incoming activation. Uncertain persistence never restores write authority, and restart reloads the committed generation. Quiescence tests cover concurrent operation rejection, fence release before reactivation, cancellation cleanup, and stalled-writer timeouts. These are filesystem-boundary tests, not proof of power-loss durability on every platform.

## Maintainer record

| Field | Result |
| --- | --- |
| Revision | Not yet recorded |
| Desktop OS and version | Not yet recorded |
| Android device, OS, and WebView | Not yet recorded |
| Result | Pending physical run |
| Exceptions | None recorded |

Do not mark this passed from an emulator-only or source-only run.
