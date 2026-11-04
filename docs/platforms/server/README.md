# Self-hosted server

**Status: Implemented in source.** The optional `ganbaru-server` program runs on a machine you control. Relay mode, anchor mode, and their Settings exist; physical acceptance is pending.

This is the setup guide. Every step is a command you can paste, so you can also hand this page to an AI assistant. [Sync](../../data/sync.md) owns the behavior and [Network and privacy](../../data/security/network-and-privacy.md) owns the security boundary.

## Do you need a server?

| Your devices | What to do |
| --- | --- |
| On the same network | Nothing. Linked devices find each other. |
| In different places, and a computer of the vault is usually on | Put every device on the same VPN (see [Reach it from other places](#reach-it-from-other-places)). Ganbaru reaches VPN addresses like local ones, with no server. |
| Rarely online at the same time, or no computer stays on | Run a server. It also lets a new computer join the vault with only a code. |

## Anchor or relay

Pick one mode per server, and one server per vault.

- **Anchor** (recommended on hardware you own, such as a home server, NAS, or old PC): a full, always-on copy of the vault. It keeps every device syncing while your computers are off, takes the anchor role, and lets a new computer [join through it](#adding-a-desktop-through-the-server). It holds the vault keys and can read the whole vault, so encryption does not protect the vault from that machine.
- **Relay** (for a rented server): stores encrypted changes until your devices pick them up. It cannot read them; it sees only sizes, timing, and opaque ids. It keeps at most 2 GiB per vault for 90 days (`--quota-mib`, `--max-age-days`).

## What you need

- A Linux machine that stays on, x86_64 or 64-bit ARM. A relay fits the smallest rented server; an anchor needs disk for the whole vault.
- TCP port 43823 reachable from your devices, through a VPN or a forwarded port. No domain or certificate is needed: devices pin the server's certificate. Do not put a TLS-terminating proxy in front of it; plain TCP forwarding is fine.
- If the machine has a firewall (Fedora and many servers turn one on), allow the port: `sudo ufw allow 43823/tcp`, or `sudo firewall-cmd --permanent --add-port=43823/tcp && sudo firewall-cmd --reload`. Docker usually opens published ports by itself. `setup` prints the same reminder, and the app says so when it cannot reach the server.
- The same version as your app. Ganbaru updates itself, so let the server update too ([Updates](#updates)).

Below, `<address>` is an address your devices can reach, and `anchor` can be replaced by `relay`. `ganbaru-server help` lists every command.

## Run it with Podman

Podman 5 or later is recommended: it runs the server without root and updates it automatically.

1. Create the data folder and the service file:

   ```sh
   mkdir -p -m 700 ~/ganbaru-server ~/.config/containers/systemd
   cat > ~/.config/containers/systemd/ganbaru-server.container <<'EOF'
   [Container]
   ContainerName=ganbaru-server
   Image=ghcr.io/opengrimoire/ganbaru-server:latest
   Exec=anchor
   PublishPort=43823:43823
   Volume=%h/ganbaru-server:/data:Z
   UserNS=keep-id:uid=65532,gid=65532
   AutoUpdate=registry

   [Service]
   Restart=always

   [Install]
   WantedBy=default.target
   EOF
   ```

2. Start it, keep it running after you log out, and turn on daily updates:

   ```sh
   systemctl --user daemon-reload
   systemctl --user start ganbaru-server
   sudo loginctl enable-linger "$USER"
   systemctl --user enable --now podman-auto-update.timer
   ```

3. Print a setup code, then [add the server to your vault](#add-the-server-to-your-vault):

   ```sh
   podman exec ganbaru-server /usr/local/bin/ganbaru-server anchor setup --endpoint <address>:43823
   ```

## Run it with Docker

1. Create the data folder. The container runs as user 65532, which must own it:

   ```sh
   sudo install -d -o 65532 -g 65532 -m 700 /srv/ganbaru-server
   ```

2. Start the server. It comes back after a reboot:

   ```sh
   sudo docker run -d --name ganbaru-server --restart unless-stopped \
     -p 43823:43823 -v /srv/ganbaru-server:/data \
     ghcr.io/opengrimoire/ganbaru-server:latest anchor
   ```

3. Print a setup code, then [add the server to your vault](#add-the-server-to-your-vault):

   ```sh
   sudo docker exec ganbaru-server /usr/local/bin/ganbaru-server anchor setup --endpoint <address>:43823
   ```

On SELinux systems such as Fedora, add `:Z` to the end of the `-v` value. A NAS container screen needs the same settings: the image, the `anchor` or `relay` command, port 43823, a folder at `/data` owned by user 65532, and a restart policy.

## Run the binary with systemd

Download the binary for your release with its checksums (use `linux_aarch64` on 64-bit ARM), then create a user and a service:

```sh
base=https://github.com/opengrimoire/ganbaru/releases/download/app-v<version>
curl -LO "$base/ganbaru-server_<version>_linux_x86_64" -LO "$base/SHA256SUMS"
sha256sum --check --ignore-missing SHA256SUMS
sudo install -m 755 ganbaru-server_<version>_linux_x86_64 /usr/local/bin/ganbaru-server
sudo useradd --system --home-dir /var/lib/ganbaru-server --shell /usr/sbin/nologin ganbaru-server
sudo tee /etc/systemd/system/ganbaru-server.service >/dev/null <<'EOF'
[Unit]
Description=Ganbaru server
After=network-online.target
Wants=network-online.target

[Service]
User=ganbaru-server
StateDirectory=ganbaru-server
StateDirectoryMode=0700
Environment=GANBARU_SERVER_DATA_DIR=/var/lib/ganbaru-server
ExecStart=/usr/local/bin/ganbaru-server anchor
Restart=on-failure
NoNewPrivileges=true
PrivateTmp=true
ProtectHome=true
ProtectSystem=strict

[Install]
WantedBy=multi-user.target
EOF
sudo systemctl daemon-reload
sudo systemctl enable --now ganbaru-server
sudo -u ganbaru-server GANBARU_SERVER_DATA_DIR=/var/lib/ganbaru-server ganbaru-server anchor setup --endpoint <address>:43823
```

## Reach it from other places

- **A VPN (safest, recommended).** Put the server and every device on the same VPN, and use the server's VPN address as `<address>`. No port is open to the Internet. [Tailscale](https://tailscale.com) is the easiest. [Headscale](https://headscale.net) (a self-hosted Tailscale server) and [NetBird](https://netbird.io) are fully open and self-hostable, and plain [WireGuard](https://www.wireguard.com) works too. All are fast. Phones need the VPN app on. If the server has a firewall, allow the port on the VPN, for example `sudo ufw allow in on tailscale0 to any port 43823 proto tcp`.
- **A forwarded port.** Forward TCP 43823 on your router to the server, or open it on a rented server, for example with `sudo ufw allow 43823/tcp`. Use a domain name you control, or a dynamic DNS name if your home address changes. Traffic stays encrypted and pinned, and the server limits connections from unknown addresses ([details](../../data/security/network-and-privacy.md)).

A setup code holds up to four addresses. Give the local one first, because many routers cannot reach their own public address from inside: `--endpoint 192.168.1.20:43823 --endpoint <address>:43823`. The addresses are part of the code, so changing them later means removing the server and adding it again.

## Add the server to your vault

A setup code works for 10 minutes and one vault. Print a new one when it expires.

1. On a desktop that is an admin of the vault, open Settings > Data, paste the code in the server section, and confirm the warning. A desktop linked to nothing can add one too.
2. For an anchor, keep **Make this server the anchor** checked. If the current anchor device is off, the desktop waits for it; once it is on, enter the same code again, or a new one if it expired.

Every linked device then learns the server and dials it from any network.

An anchor takes the anchor role only when it is added. If a desktop later takes the anchor, or you kept it on a desktop, the server keeps serving but cannot take it back: remove the server and add it again on an empty data folder. The recovery kit also cannot be regenerated while a server is the anchor; take the anchor to a desktop first.

## Adding a desktop through the server

A new computer, such as a work PC or one replacing every lost device, can join through an anchor server without meeting another device.

1. Run `anchor setup` on the server for a fresh code.
2. On the new computer, choose **or connect this computer to another computer or a server** on the first launch, or **Connect to a computer or server** in Settings > Data. Open **A server** and paste the code. The server adds the computer as an admin and it copies the vault.
3. If the server refuses, for example because its admin key was removed, choose **Use the recovery kit instead** and enter the kit too.

The code alone is enough because printing it takes a shell on the server, which can already read the vault. The new computer opens the copy only when the vault's log adds this computer's own keys. Until a linked device shares the person key with it, it cannot sign contact cards or requests. If the copy fails, repeat with a fresh code.

## Updates

The app updates itself, and a server that falls behind stops working with it once the sync format changes, so keep the server updating too:

- **Podman** with the file above checks `latest` every day, and returns to the previous image if the new one does not start.
- **Docker:** `sudo docker pull ghcr.io/opengrimoire/ganbaru-server:latest`, then `docker stop`, `docker rm`, and the same `docker run`.
- **Binary:** replace it as in [Run the binary with systemd](#run-the-binary-with-systemd) and `sudo systemctl restart ganbaru-server`.

To choose when you update, use `:<version>` or `@sha256:<digest>` (from the release job summary) in place of `:latest`.

Each start upgrades the data folder in one transaction per step, so a failed upgrade leaves it as it was. After a newer version has run, an older one cannot use the folder. Do not restore an old copy of an anchor's folder over one your devices used: it signs with the same keys, and devices treat that as a fork. To recover from a bad release, update to the fixed one.

## Stopped commands

`relay remove-space` and `anchor backup` need a stopped server. Run them in a temporary container that shares the folder (Podman: `systemctl --user stop ganbaru-server`, then `podman run --rm -v ~/ganbaru-server:/data:Z --userns keep-id:uid=65532,gid=65532 ghcr.io/opengrimoire/ganbaru-server:latest <command>`, then start it again; Docker: the same with `sudo docker` and `/srv/ganbaru-server`; systemd: stop the service and run the command as `ganbaru-server` with `GANBARU_SERVER_DATA_DIR` set). `relay setup`, `relay spaces`, and `relay revoke-setup` work while it runs.

## Backups

An anchor's vault also lives on your devices. For a file that restores on any new device with the recovery kit, run `anchor backup --out /data/vault.ganbaru-backup` as a stopped command, copy the file off the server, and delete it there. A copy of the data folder is not a backup for another machine; copy it only to move the server, and never run both copies.

A relay holds only copies of what your devices hold. Without its folder, run a fresh relay, remove the old one in Settings, and add the new one.

## Removing a server

On an admin desktop, take the anchor first if the server holds it. Remove the server in Settings > Data, and remove an anchor from the device list too. Then stop the server and delete its data folder. A relay deletes what it stores for the vault when the desktop can reach it.
