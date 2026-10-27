# Network and privacy

Ganbaru has no analytics, advertising, telemetry, crash-reporting, or behavioral-tracking service. It sends no calendar, focus, Notes, project, browsing-control, or Chat data to project-operated infrastructure. Offline use is a first-class mode, and every network-capable feature is explicit, bounded, and attributable to a user choice or configured integration.

## Current network surfaces

Current code uses the network only for these feature-owned purposes:

- Update checks: the desktop updater and, on Android, the GitHub releases API.
- User-selected coding-agent providers, whose native processes contact their configured service.
- Chat hosted source control: GitHub, GitLab, and Azure DevOps through their user-installed CLIs, and Bitbucket through its API with a stored credential.
- YouTube playback, metadata, and thumbnails for the Music player. Metadata and thumbnail lookups share one client per purpose, refuse redirects, time out, run under a global concurrency bound, and abandon a response body as soon as it exceeds its size cap instead of buffering it.
- User-initiated Notion API import and remote project-icon import from a user-supplied URL. Project icon requests use HTTPS only: an `http` URL, typed or reached by redirect, is upgraded to `https` before any request, so the icon never travels in plain text.
- Chat browser previews and URLs explicitly opened by the user.
- Local LAN device linking and whole-vault handoff, with pinned certificates and mutual TLS (see [Synchronization](../sync.md)). The desktop listener binds the IPv4 and IPv6 wildcard addresses and drops every peer outside the local network ranges below right after accept, before the connection budget, TLS, or any read. Invitations and contact cards name one reportable local address: the IPv4 source of the default route, otherwise another private or overlay IPv4 address, otherwise an IPv6 unique local address. A desktop starts the listener at launch only when it has linked devices, a transfer in flight, or a person identity whose handed-out card may name it; otherwise it starts when the user creates a linking invitation, shows or regenerates their contact card, or sends a contact request. The listener bounds connections before they cost work: connections without a linked, unrevoked device certificate share a small global budget and a smaller budget per IPv4 address or IPv6 /64, linked devices draw from a separate budget so a flood from one host cannot lock them out, frames before authentication are capped at 16 KiB, handshakes and idle reads time out, and transient accept errors back off instead of stopping the listener. A linked device refused for a full budget sees a retryable busy error.
- Concurrent sync of the replicated domains (Quick notes and their tags, portable preferences, user themes, and contacts) on the same listener. Every sync request is answered only after the peer is authenticated as an enrolled device of the vault it names; the coordinator stores and serves operations and accepts a new writer only when its certificate names the authenticated device. Requests carry the vault and device identifiers, version vectors, operation hashes, signed operations that contain values of replicated tables with the writer certificates, and the bytes of managed assets of the registered classes (Quick note images, the profile picture) that those operations reference; no other vault data travels this way. Asset bytes move only between a linked device and the coordinator over this link, never to a third party. Operations are signed but not separately encrypted, so the mutual TLS link is their only confidentiality protection.
- LAN peer exchange between members (see [Exchange](../sync.md#exchange)). A linked member desktop listens on the coordinator port while sync runs, with the same local network admission, connection budgets, and timeouts as the coordinator listener. It requires a client certificate and accepts only the fingerprints of devices the local selected authorization log enrolled and did not remove and that this installation did not quarantine; anything but a sync request, including pairing, handoff, contact, and proposals, is refused with a typed code. On Linux the listener admits nothing on a network until its firewall rule is confirmed. Members dial each other only at reported local network endpoints, pinned to the certificate the log recorded for that device, and the peer book of those endpoints stays device-local.
- Contact requests and status polls on the same LAN listener. With enrollment requests, which must carry the invitation secret and link nothing until the coordinator user approves the matching code (see [Synchronization](../sync.md)), they are the only inbound messages accepted without a client certificate (see [Contacts and contact requests](../sync.md#contacts-and-contact-requests)); an enrollment request that presents a client certificate is refused. The surface is bounded: a card is at most 512 bytes and must carry a valid signature before anything is written, the recipient card nonce is compared in constant time, status polls are signed and rejected outside a five-minute window, pending received requests are capped at 64 with the oldest deleted first, accepted, declined, and expired requests are kept for 30 days and at most 512 at once, only pending requests are listed, and a blocked requester receives the same response as an accepted one, no sooner: every handled contact request is answered at least 250 ms after it arrives, so the skipped database write cannot reveal a block through timing. Contact messages are handled by a bounded worker apart from the coordinator actor, so they cannot delay handoff or sync; each source address (IPv4 host or IPv6 /64) spends a budget of 16 messages refilled at one per second, and a source past its budget or a full worker queue gets a retryable `recipient_busy` answer. Unauthenticated handling reads the identity row; the person key is loaded from the operating-system keyring only to answer an accepted status poll with the signed recipient card. Outbound, the requester connects only to the endpoint and certificate fingerprint named in the card it was handed.
- LAN discovery of the coordinator over DNS-SD on multicast DNS (UDP 5353). The desktop coordinator advertises `_ganbaru._tcp` while its listener runs and the active vault has a linked device; a linked desktop browses for at most 3 seconds, at most once a minute, and only after every known coordinator endpoint failed. An Android background pass that reached no endpoint browses through the system `NsdManager` for at most 3 seconds and resolves only the expected instance names; Android never advertises. An advertisement carries the listener port, at most eight reportable addresses (never loopback or IPv6 link-local), the TXT record `v=1`, a host name drawn at random once per app run, and an instance name that is an HMAC-SHA256 member tag over the vault's discovery key, the coordinator certificate fingerprint, and the current hour. Browsers accept only the names expected for the previous, current, and next hour. A non-member learns that a Ganbaru coordinator runs on the network, its port, and its LAN addresses, but not the device name, the vault, or the certificate, and cannot link one hour's advertisement to the next. The responder never answers on loopback and does not run in the root firewall helper.
- Outgoing linking and contact connections dial only local network addresses, because invitations and cards arrive as untrusted QR codes or text, and the same ranges govern listener admission, the addresses a coordinator reports, and the discovered addresses a linked device dials: private IPv4, IPv4 link-local, the `100.64.0.0/10` overlay range used by Tailscale and carrier-grade NAT, IPv6 unique local and link-local, and global IPv6 inside the /64 of the source address this device would use to reach it (for an inbound peer, the /64 of the address the connection arrived on). Loopback is dialed and admitted only by development builds. Public, documentation, multicast, broadcast, and unspecified addresses are refused before any packet is sent, and invitations carrying them fail validation. Linking across the internet belongs to the planned relay.
- The Chrome extension's native-messaging connection, which is local and not a remote service.

Each adapter owns its allowed schemes, origins, redirects, response bounds, timeouts, and credential handling. There is no generic command that fetches an arbitrary URL with application credentials.

Provider network behavior follows the selected provider and its own policy. Ganbaru constrains the process, workspace, environment, host tools, and context it supplies, but cannot promise that an external provider does not retain submitted content. The UI must make provider choice and data transfer understandable.

## Planned network surfaces

Not implemented:

- Concurrent sync of the remaining domains, LAN peer exchange between enrolled devices, operation encryption, and optional remote synchronization through a user-hosted Rust relay or always-on anchor.
- A hosted or local BYOK chat widget separate from coding-agent Chat.
- A separately authorized external MCP service.
- A `ganbaru` CLI with external integration commands.

Their security requirements are design constraints. Documentation must not describe their proposed endpoints as current traffic.

## No hidden telemetry

Logs, diagnostics, and benchmark results stay local unless the user explicitly exports or copies them. Update and icon requests carry no unrelated vault content.

Do not add analytics SDKs, remote feature flags, session replay, tracking pixels, or automatic diagnostic uploads. A future optional diagnostic submission would need a separate design with preview, redaction, explicit consent, and a stated recipient.

## URL and request rules

Network adapters validate:

- Scheme and hostname allowlist.
- Redirect destination and count.
- Credentials embedded in URLs.
- Loopback, private, link-local, multicast, and unspecified addresses.
- Request and response size.
- Content type and parser limits.
- Connect, header, body, and inactivity deadlines.
- Secret placement in headers, query strings, logs, and errors.

An origin permitted by CSP is not automatically permitted by a Rust adapter, and an opener allowlist does not grant fetch authority.

Activated Notes hyperlinks use a feature-owned native command on desktop and Android. It applies the rich-text URL bounds, allows only HTTP, HTTPS, and validated email links, and rejects embedded credentials before opening the system handler. Opening a hyperlink does not fetch its content through the app or widen the opener allowlist.

## Local data encryption

The vault and SQLite database are not application-encrypted. Ganbaru relies on operating-system account isolation and full-disk encryption such as LUKS, BitLocker, or FileVault.

Why: this keeps user files accessible to ordinary backup and editing tools and avoids a second fragile key-recovery system. The cost is that malware running as the same user can read the vault, and documentation must state that limitation honestly.

Secrets are different. Provider credentials live behind operating-system credential references and are materialized only at the native operation that needs them. They never belong in `config.json`, the vault database, provider DTOs, diagnostics, or exports.

Sync writer keys are per installation and per vault. Desktop stores them in the operating-system keyring; Android stores them in private application files, with the same protection as the person key, until Keystore wrapping is implemented. Neither location is inside the vault, so writer keys never enter snapshots, backups, or device transfers. A restored or copied database is detected and gets a new writer instead of reusing an old key's sequence.

Sync device and admin keys follow the same per-installation, per-vault rule and are never copied between devices; only desktops hold admin keys. The recovery seed, which derives the recovery admin key that can promote a desktop to admin, stays in the desktop keyring only until the user confirms saving the recovery kit, then is deleted, so afterwards only the kit the user keeps can sign with it. Until then the desktop can show the kit code again; the settings confirmation that ends this lands with the authority UI. Using the kit derives the key in memory for one proposal and never stores it, and no debug output shows a kit code.

The device linking TLS key follows the same rule. Desktop moves it from the private pairing state file into the operating-system keyring the first time linking needs it, and keeps it in the private file only when no keyring accepts it, so keyring-less Linux desktops can still link. Android keeps it in the private file until Keystore wrapping is implemented. A key that disappears from the keyring is never silently replaced while peers pin this device, because a new certificate would break every link; the device reports the problem and is relinked by the user.

## Future synchronization encryption

**Planned.** Remote sync must use end-to-end encryption in addition to transport encryption. The accepted design uses signed, encrypted resource-scoped operations and immutable asset chunks, delivered directly or through an optional user-hosted Rust relay that stores opaque records.

The target composition uses pinned TLS 1.3 device identities, XChaCha20-Poly1305 records, and HPKE with X25519 and HKDF-SHA256 for resource keys. It requires single-use invitations confirmed by the administration device, a separate recovery identity, signed membership history, key rotation on revocation, fresh writer generations on rejoin, replay protection, and downgrade rejection. A dependency audit is not a protocol security review.

The relay must never receive plaintext. It can still observe connection metadata, timing, ciphertext size, and routing identifiers unless later padding work reduces them. Revocation prevents future operations and key distribution but cannot erase plaintext already on an offline device or external provider. See [Synchronization](../sync.md).

## Data minimization

Every request carries the smallest data its feature needs:

- Update checks include no vault identity or activity history.
- Provider prompts include only explicitly selected and authorized context.
- Media requests include no Notes, calendar, or project data.
- Icon imports send only the user-supplied URL.
- Future availability sharing uses coarse user-approved capacity, not raw Pomodoro or idle history.

Derived data stays sensitive. A summary, embedding, screenshot, or search result can reveal the same information as its source and follows the same authorization and transfer policy.

## External content

Remote images, media metadata, HTML, Markdown, JSON, XML, provider events, and downloaded files are untrusted and are parsed by the narrowest feature adapter with bounds. Remote content cannot choose a local path, execute a command, access credentials, or create an organizational grant.

## User disclosure

Before a feature sends personal or repository content to an external service, the product should identify the provider, purpose, selected context, and material privacy consequence. Persistent background access requires a clear setting and a way to disable it. Disabling an integration stops new requests and cleans up local credentials according to its retention contract; it cannot recall data already sent.
