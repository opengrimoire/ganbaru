# Self-hosted server

**Status: Implemented in source.** The `ganbaru-sync-relay` binary runs on a machine you control, and sync never needs it. Relay mode, anchor mode, and their Settings exist; physical acceptance is pending.

This is the setup guide. [Sync](../../data/sync.md) owns the behavior and [Network and privacy](../../data/security/network-and-privacy.md) owns the security boundary.

## Do you need a server?

Linked devices sync with each other on the same network, and over a private overlay network, without any server.

Tailscale alone is enough when every device runs it on the same tailnet and a desktop of the vault is on when the others sync. Ganbaru dials Tailscale addresses like local ones, so there is nothing to set up. Devices that are never online together still need a server, because each one can only hand changes to a device that is awake.

Run a server when your devices are often apart and no desktop stays on, or when a phone should sync without a VPN. A server can also sit behind Tailscale: name its Tailscale address in the setup command and forward no port.

## Relay or anchor

Pick one mode for a data directory, and one server for a vault.

| Mode | What it does | What the server can read | Run it on |
| --- | --- | --- | --- |
| Relay | Stores sealed records and asset chunks between devices that are not online together. | No record content and no key. It sees record size and timing, the space and writer they belong to, and when the vault's list of key recipients changes: how many there are and an opaque id for each, which show when a device joins or leaves. | A rented server or hardware you own. |
| Anchor | Runs a full replica of one vault, serves every device, and takes the anchor role, so the vault keeps moving while your personal devices sleep. It also holds an admin key and sequences authority for the vault, and it is a backup. | The whole vault. It holds the space keys. | Hardware you own. |

An anchor server keeps the vault in plaintext and holds its keys, so encryption does not protect the vault from that machine or from whoever runs it. Use relay mode on a rented server, and anchor mode on hardware you own. Disk encryption on the server protects an anchor only while the machine is off.

## What you need

- A small Linux machine that stays on. A relay fits the smallest virtual server you can rent. An anchor needs disk for a whole vault.
- A data directory only the server user can read. All server state lives in that directory.
- TCP port 43823 reachable from your devices, through a public address, a forwarded port, or a VPN address. Devices pin the server's certificate fingerprint, so no domain or certificate authority is needed. Do not put a proxy that terminates TLS in front of the server, because devices would refuse its certificate; plain TCP forwarding is fine.
- A linked vault. A server only serves a vault that syncs, so link two devices on the same network first, and add the server from an admin desktop.
- The [container image or the binary](#get-the-server) of the same version as your app.

Commands below use `<version>` for that version and `<address>` for a name or address your devices can reach. Prefer a domain name that you control: the address is part of the setup code, so changing it later means removing the server and adding it again. `ganbaru-sync-relay help` lists every option.

## Get the server

Containers need nothing else. The image is `ghcr.io/opengrimoire/ganbaru-sync-relay:<version>`, there is no `latest` tag, and the release job summary records its digest. To be sure of what you run, use `ghcr.io/opengrimoire/ganbaru-sync-relay@sha256:<digest>` in place of the tag.

For the bare binary, download it with the checksum files from the [release](../../operations/release/distribution.md#sync-relay). Use `linux_aarch64` in place of `linux_x86_64` on 64-bit ARM.

```sh
base=https://github.com/opengrimoire/ganbaru/releases/download/app-v<version>
curl -LO "$base/ganbaru-sync-relay_<version>_linux_x86_64"
curl -LO "$base/SHA256SUMS"
sha256sum --check --ignore-missing SHA256SUMS
sudo install -m 755 ganbaru-sync-relay_<version>_linux_x86_64 /usr/local/bin/ganbaru-sync-relay
```

## Run it on a rented server with Docker

Use relay mode here.

1. [Install Docker](https://docs.docker.com/engine/install/), and allow TCP 43823 in the provider's firewall and on the server, for example with `sudo ufw allow 43823/tcp`.
2. Create the data directory. The container runs as user 65532, which must own it.

   ```sh
   sudo install -d -o 65532 -g 65532 -m 700 /srv/ganbaru-server
   ```

3. Start the relay. It comes back after a reboot.

   ```sh
   sudo docker run -d --name ganbaru-server --restart unless-stopped \
     -p 43823:43823 -v /srv/ganbaru-server:/data \
     ghcr.io/opengrimoire/ganbaru-sync-relay:<version>
   ```

4. Print a setup code, then continue with [Relay mode](#relay-mode).

   ```sh
   sudo docker exec ganbaru-server /usr/local/bin/ganbaru-sync-relay \
     setup --data-dir /data --endpoint <address>:43823
   ```

On a distribution with SELinux, such as Fedora, add `:Z` to the end of the `-v` value.

## Run it on a NAS or home server

Use anchor mode here when you own the machine, or relay mode if you prefer.

1. Give the machine a fixed address on your network in the router's settings.
2. Forward TCP 43823 on the router to that address. Skip this if the devices reach the machine through Tailscale.
3. If your home address changes, set up a dynamic DNS name and use it as `<address>`.
4. Start the server with Docker as above, or the systemd unit below. A NAS with a container screen needs the same four settings: the image, port 43823, a folder mounted at `/data` and owned by user 65532, and the restart policy. For an anchor, replace the command with `anchor --data-dir /data --listen 0.0.0.0:43823`:

   ```sh
   sudo docker run -d --name ganbaru-server --restart unless-stopped \
     -p 43823:43823 -v /srv/ganbaru-server:/data \
     ghcr.io/opengrimoire/ganbaru-sync-relay:<version> \
     anchor --data-dir /data --listen 0.0.0.0:43823
   ```

5. Print a setup code with both addresses, the local one first, because many routers cannot reach their own public address from inside. Then continue with [Relay mode](#relay-mode) or [Anchor mode](#anchor-mode).

   ```sh
   sudo docker exec ganbaru-server /usr/local/bin/ganbaru-sync-relay \
     anchor setup --data-dir /data --endpoint 192.168.1.20:43823 --endpoint <address>:43823
   ```

   For a relay, use `setup` in place of `anchor setup`. A code holds at most four addresses.

## Run the binary with systemd

1. Create a user for the server.

   ```sh
   sudo useradd --system --home-dir /var/lib/ganbaru-server --shell /usr/sbin/nologin ganbaru-server
   ```

2. Save this unit as `/etc/systemd/system/ganbaru-server.service`. For an anchor, replace `serve` with `anchor` in `ExecStart`.

   ```ini
   [Unit]
   Description=Ganbaru sync server
   After=network-online.target
   Wants=network-online.target

   [Service]
   User=ganbaru-server
   Group=ganbaru-server
   StateDirectory=ganbaru-server
   StateDirectoryMode=0700
   ExecStart=/usr/local/bin/ganbaru-sync-relay serve --data-dir /var/lib/ganbaru-server
   Restart=on-failure
   NoNewPrivileges=true
   PrivateTmp=true
   ProtectHome=true
   ProtectSystem=strict

   [Install]
   WantedBy=multi-user.target
   ```

3. Start it, and print a setup code as the same user.

   ```sh
   sudo systemctl daemon-reload
   sudo systemctl enable --now ganbaru-server
   sudo -u ganbaru-server ganbaru-sync-relay setup --data-dir /var/lib/ganbaru-server --endpoint <address>:43823
   ```

   For an anchor, use `anchor setup` in place of `setup`.

## Relay mode

1. Print a setup code on the server, as shown above. The command prints a QR code and a text code. The code works once, for one vault, for 15 minutes.
2. Keep the anchor desktop online, and enter the code in Settings > Data, in the server section of an admin desktop.
3. Confirm that you understand the server encryption has not had an external security review yet, and add the server. It takes effect once the anchor confirms it.

The relay keeps at most 2 GiB per vault for 90 days. Change that with `--quota-mib` (at least 64) and `--max-age-days` (at least 1) on `serve`. A device away for longer catches up from another device directly.

`spaces` lists the vaults the relay stores, and `revoke-setup` cancels unused codes, or one with `--code`. Both work while the relay runs. `remove-space --space <id>` deletes one vault and needs a stopped relay, as in [Upgrades](#upgrades-and-stopped-commands).

## Anchor mode

An anchor serves one vault. The desktop that adds it must be an admin.

1. Print a setup code on the server, as shown above, naming each address your devices can reach; a phone away from home needs a public or VPN address. The code works once, for 15 minutes. A newer code replaces an older one, so run it again whenever a code expires.
2. Enter the code in Settings > Data, in the server section of an admin desktop, and confirm that this server can read your whole vault. The desktop proves the code, learns the server's keys, proposes the server's enrollment as an admin, and uploads a copy of the vault. No admin key or person key leaves the desktop, and the server dials nobody.
3. Keep the default to let the server take the anchor, or choose to keep the current anchor. If the anchor desktop is off, the desktop waits for it; once it has run, enter the same code again, or a new one if it expired.

Every linked device learns the server from the log and dials it from any network. Two dials happen before that: the desktop adding the server dials the endpoints in the setup code, which may be public, and recovery with the kit through an anchor server dials them from a new device, which may be off the LAN. Every other new device links to a desktop first, on the LAN.

Two limits apply today. A server takes the anchor only as part of being added, so once a desktop takes the anchor from it, or you kept the anchor on a desktop, the anchor cannot go to that server: it keeps running its vault and refuses a second upload with "this server already runs a vault". To return to a server anchor, [remove the server](#removing-a-server) and add it again with a fresh setup on an empty data directory. The recovery kit also cannot be regenerated while a server anchors, so take the anchor to a desktop, regenerate the kit there, and return the same way.

## Upgrades and stopped commands

Keep the server on the same version as the app. An older server asks for an update when you add it, and one that lags behind keeps serving the records it holds until the handoff protocol itself changes. For a relay, Settings shows when the server and the device speak different protocol versions; an anchor server is not probed for this. The data directory stays across upgrades, and the server upgrades it when it starts, so an older binary cannot be put back afterwards. The server never updates itself.

With Docker, stop and remove the container, then run the same `docker run` command with the new version:

```sh
sudo docker stop ganbaru-server
sudo docker rm ganbaru-server
```

With systemd, replace the binary as in [Get the server](#get-the-server), then run `sudo systemctl restart ganbaru-server`.

Commands that need a stopped server run the same way. With Docker, stop the container and start a temporary one that shares the folder, then start the server again:

```sh
sudo docker stop ganbaru-server
sudo docker run --rm -v /srv/ganbaru-server:/data ghcr.io/opengrimoire/ganbaru-sync-relay:<version> \
  remove-space --data-dir /data --space <id>
sudo docker start ganbaru-server
```

With systemd, use `sudo systemctl stop ganbaru-server`, run the command with `sudo -u ganbaru-server`, then `sudo systemctl start ganbaru-server`.

## Backups

A relay holds only copies of what your devices hold, so a backup is optional. To keep its identity, copy the data directory while the relay is stopped, for example with `sudo tar -czf ganbaru-server.tar.gz -C /srv ganbaru-server`, and restore it with its owner. Without a copy, run a fresh relay, remove the old one in Settings, and add the new one.

For an anchor, stop the server and write a backup file. With Docker:

```sh
sudo docker stop ganbaru-server
sudo docker run --rm -v /srv/ganbaru-server:/data ghcr.io/opengrimoire/ganbaru-sync-relay:<version> \
  anchor backup --data-dir /data --out /data/vault.ganbaru-backup
sudo docker start ganbaru-server
```

With systemd, stop the service and run `sudo -u ganbaru-server ganbaru-sync-relay anchor backup --data-dir /var/lib/ganbaru-server --out /var/lib/ganbaru-server/vault.ganbaru-backup`. Copy the file off the server and delete it there. The vault needs a recovery kit. The file is encrypted to that kit and restores on any new device, like a backup made in the app.

Copying an anchor's data directory is not a backup of the vault for another machine, because its keys identify this server, and it holds the vault in plaintext. Copy it only to move the server: stop the old one, copy the whole directory with its owner, and never start the old one again. Two copies running at once sign with the same keys, and devices treat that as a fork. Do not put an older copy back over a server that devices still use.

## Recovering after losing every device

The server must hold the anchor role, and you need the recovery kit.

1. Run `anchor setup` on the server for a fresh code.
2. On a new desktop, choose **Recover from an anchor server** on the linking screen of the first launch, or in Settings > Data when the device is not linked, and enter the code and the kit. The desktop derives the recovery admin key in memory, enrolls itself as an admin through the server, and copies the vault into a new folder.
3. Keep the recovery kit. The person key does not return this way: a linked phone or desktop that still holds it can share it again, and otherwise anything signed with it stays as it is.

If the copy fails after the enrollment, repeat the steps with a fresh code and the same kit.

## Removing a server

On an admin desktop, take the anchor first if the server holds it. Then remove the server in Settings > Data, remove an anchor from the device list, stop the process, and delete the data directory. A relay deletes what it stores for the vault when the desktop can reach it; otherwise run `remove-space`, or delete the data directory.
