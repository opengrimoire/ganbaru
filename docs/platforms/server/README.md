# Self-hosted server

**Status: Partial.** The `ganbaru-sync-relay` binary runs on a machine you control, and sync never needs it. Relay mode and anchor mode are implemented in source; physical acceptance is pending. Settings for adding a server are in progress, so the app steps below name what the desktop does once they ship.

Pick one mode for a data directory.

| Mode | What it does | What the server can read |
| --- | --- | --- |
| Relay | Stores sealed records and asset chunks between devices that are not online together. | Nothing inside the records. |
| Anchor | Runs a full replica of one vault, serves every device, and takes the anchor role, so the vault keeps moving while your personal devices sleep. It is also a backup. | The whole vault. It holds the space keys. |

Choose the anchor only for a machine you trust with your vault. [Sync](../../data/sync.md) owns the behavior and [Network and privacy](../../data/security/network-and-privacy.md) owns the security boundary.

## What you need

- A small Linux machine or virtual server that stays on, and a data directory only the server user can read. Back up nothing else: every piece of server state lives in that directory.
- A port reachable from your devices, such as a forwarded port, a VPN address, or a public address. Devices dial the addresses you give the setup command, and they pin the server's certificate fingerprint, so no certificate authority or domain is required.
- The binary or the container image from a [release](../../operations/release/distribution.md#sync-relay).

## Relay mode

1. Start the relay with `ganbaru-sync-relay serve --data-dir <path>`.
2. Run `ganbaru-sync-relay setup --data-dir <path> --endpoint <host:port>` and enter the code on an admin desktop. The code works once, for 15 minutes.

`spaces`, `remove-space`, and `revoke-setup` manage what the relay stores. `ganbaru-sync-relay help` lists every option.

## Anchor mode

An anchor serves one vault. The desktop that adds it must be an admin.

1. Start the server with `ganbaru-sync-relay anchor --data-dir <path>`. It listens on port 43823 unless you pass `--listen <address:port>`.
2. In a second shell, run `ganbaru-sync-relay anchor setup --data-dir <path> --endpoint <host:port>`. Name each address your devices can reach; a phone away from home needs a public or VPN address. The command prints a QR code and a text code that work once, for 15 minutes. A newer code replaces an older one, so run it again whenever a code expires.
3. Enter the code in Settings > Data, in the server section of an admin desktop. The desktop proves the code, learns the server's keys, proposes the server's enrollment as an admin, and uploads a copy of the vault. No admin key or person key leaves the desktop, and the server dials nobody.
4. Keep the default to let the server take the anchor, or choose to keep the current anchor. If the anchor desktop is off, the desktop waits for it; enter the same code again once it has run. The code stays valid until the server activates.

Every linked device learns the server from the log and dials it from any network. New devices still link to a desktop first, on the LAN.

An older server answers with a request to update it. Replace the binary and restart it; the data directory stays. A server that lags behind the app keeps serving the records it holds until the handoff protocol itself changes.

## Backups

Stop the server, then run `ganbaru-sync-relay anchor backup --data-dir <path> --out <file>`. The file is encrypted to the recovery kit and restores on any new device, like a backup made in the app. Copying the data directory is not a backup of the vault for another machine, because its keys identify this server.

## Recovering after losing every device

The server must hold the anchor role, and you need the recovery kit.

1. Run `anchor setup` on the server for a fresh code.
2. On a new desktop, enter the code and the kit. The desktop derives the recovery admin key in memory, enrolls itself as an admin through the server, and copies the vault into a new folder.
3. Keep the recovery kit. The person key does not return this way: a linked phone or desktop that still holds it can share it again, and otherwise anything signed with it stays as it is.

If the copy fails after the enrollment, repeat the steps with a fresh code and the same kit.

## Removing a server

On an admin desktop, take the anchor first if the server holds it. Then remove the server in Settings > Data, remove it from the device list, stop the process, and delete the data directory.
