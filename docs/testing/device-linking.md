# Device linking acceptance

**Status: Partial.** Automated pairing, snapshot bootstrap, transport, and replication tests pass; physical desktop and Android acceptance is pending. Record the result below when a maintainer completes it. Linking and snapshot bootstrap are specified in [Sync](../data/sync.md).

## Required setup

- One supported Linux or Windows desktop and one Android 10 or newer phone on the same private LAN.
- Builds from the same revision, with a fresh Android installation available for the first pass.
- A desktop vault containing an identifiable Calendar event with an alarm and Pomodoro configuration, a Project task and managed project file, a Note and attachment, a Chat channel, a Quick note, a custom theme, a distraction usage limit and history, a Music item and playlist, and a profile or project asset. Keep one music file and one project folder outside the vault.

## Acceptance script

1. **Pairing onboarding.** After choosing a desktop vault, the linking screen opens with a QR code and renewal countdown, and the code renews automatically. `Not now` loads the app without completing onboarding, so the linking screen returns after restart. On Android, scan from a normal handheld distance; the camera stops, the success state names the desktop, and Continue offers the first copy of the linked vault. An untouched starter vault shows no replacement warning. Data settings and the top-bar control reopen the same scanner.
2. **First copy with local data.** On a clean run, skip linking, add Android-only data, then link and choose `Copy vault to this device`. A warning explains that the data cannot be merged and that a backup is kept in Settings > Data. Cancel starts nothing. Confirming copies the linked vault from the coordinator; every prepared record and asset opens on Android, Settings > Data lists the backup. Restore on that row asks for confirmation and is refused with an unlink message while the phone is linked, leaving the linked vault in use; after unlinking, it restores. Exporting it to Downloads saves a copy of the archive. With storage too full for the backup, the link is refused with a storage message and the Android data stays in use. A third replaced vault drops the oldest backup. External music and project bytes are not copied.
3. **Offline editing.** Restart both apps. With desktop unreachable, edit on Android; scheduled Calendar alarms and anti-distraction enforcement continue. Edit on the disconnected desktop too and record distraction usage there.
4. **Reconnect.** Reconnect both devices without any action. Each edit reaches the other device, both show the combined distraction usage total exactly once, and a Calendar alarm change made on desktop reaches Android's native schedule after the next exchange.
5. **Both devices write.** Edit the same Quick note, Project task, Note, and Chat conversation on both devices while they are disconnected, then reconnect. Each domain merges or shows its conflict choice, restart preserves the result, and no device ever refuses an edit because another device is linked.
6. **Interrupted copies.** Interrupt connectivity during a first copy and during a fresh copy in separate passes. Nothing replaces the vault in use before the copy is complete and validated, and an explicit retry succeeds. A copy requested while the coordinator is busy waits briefly and then reports busy without changing anything.
7. **Unlink and recovery.** A removed offline device learns on its next request that it was unlinked, stops exchanging, and keeps its copy writable. When the coordinator itself was removed, the separately confirmed recovery action unlinks this device from every peer and keeps its copy as a standalone writable vault; nothing merges.
8. **Version mismatch.** Builds with different embedded migration sets or handoff protocol versions cannot pair, and already linked mismatched devices reject a copy before any snapshot or replacement. Updating both restores linking.
9. **Desktop with local data.** Linking a desktop that already holds an independent vault offers a folder for the linked vault: the default platform folder when empty, otherwise an empty sibling named after the vault, or a chosen empty folder (a folder with files is refused). `Copy vault to this device` shows the replacement warning; confirmation opens the linked vault in that folder and leaves the old vault untouched and listed in the vault picker.
10. **Fresh copy.** Keep a linked device offline until the coordinator compacts past it. Sync status says it needs a fresh copy and offers one action; nothing starts by itself. Confirming pushes its own changes first, then copies the vault, and an edit made offline survives.

## Selection and restore cases

- Switching desktop vaults, including with an idle or break overlay visible, leaves no old overlay, tray action, or delayed playback callback able to control the new vault.
- A failed drain of native delivery keeps the active folder and reports the failure; a concurrent folder action reports busy.
- A blocked Music source during vault selection retains the track as an interruption and never plays from an old load.
- On Android, canceling the backup picker preserves the active vault. Restoring revokes old playback and notification surfaces before replacement.
- A corrupt or incomplete archive is rejected before the active vault changes, and a later valid restore proceeds.

## Connectivity cases

Run with a Linux coordinator behind UFW or firewalld, a linked Windows desktop, and a linked phone; the coordinator and the phone also run Tailscale for the overlay case.

- A new DHCP address in the same subnet: linked devices reconnect within one retry interval through the reported endpoints, with no new firewall prompt.
- The coordinator moves to another subnet while the phone is closed: the phone's remembered endpoints fail, discovery finds the coordinator within about 5 seconds, and status shows the new address.
- The coordinator on Wi-Fi and Ethernet at once: both addresses are reported, and linked devices keep using the last one that answered.
- Devices on different networks that share a Tailscale network sync over `100.x` addresses, and status says overlay network.
- A fresh firewall grant asks for the administrator password once and covers TCP and UDP on IPv4 and IPv6; the phone connects over unique local IPv6 where the network offers it.
- Another device takes the coordinator's old address: the stale endpoint fails the pinned handshake, the dial moves on, and no error is shown.
- A non-member device browsing `_ganbaru._tcp` over two hours sees two unrelated instance names and two unrelated host names.
- Windows Defender's program rule admits both listener sockets and DNS-SD, and the responder shares UDP 5353 with Avahi and the Windows mDNS service.
- Removing the installed package removes every TCP and UDP rule the app added, on IPv4 and IPv6.

## Automated durability coverage

Snapshot activation tests cover staged bundle validation, replacement copies, the Android starter and replaced-vault backup decisions, and directory sync of the replaced folder and its parent before the old folder is deleted. Quiescence tests cover concurrent operation rejection, fence release before reactivation, cancellation cleanup, and stalled-writer timeouts. These are filesystem-boundary tests, not proof of power-loss durability on every platform.

## Maintainer record

| Field | Result |
| --- | --- |
| Revision | Not yet recorded |
| Desktop OS and version | Not yet recorded |
| Android device, OS, and WebView | Not yet recorded |
| Result | Pending physical run |
| Exceptions | None recorded |

Do not mark this passed from an emulator-only or source-only run.
