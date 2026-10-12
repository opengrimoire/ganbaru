# Network and privacy

Ganbaru AI has no analytics, advertising, telemetry, crash-reporting, or behavioral-tracking service. It sends no calendar, focus, Notes, project, browsing-control, or Chat data to project-operated infrastructure. Offline use is a first-class mode, and every network-capable feature is explicit, bounded, and attributable to a user choice or configured integration.

## Current network surfaces

Current code uses the network only for these feature-owned purposes:

- Update checks: the desktop updater and, on Android, the GitHub releases API.
- User-selected coding-agent providers, whose native processes contact their configured service.
- Chat hosted source control: GitHub, GitLab, and Azure DevOps through their user-installed CLIs, and Bitbucket through its API with a stored credential.
- YouTube playback, metadata, and thumbnails for the Music player.
- User-initiated Notion API import and remote project-icon import from a user-supplied HTTPS URL.
- Chat browser previews and URLs explicitly opened by the user.
- Local LAN device linking and whole-vault handoff, restricted to a private LAN address with pinned certificates and mutual TLS (see [Synchronization](../sync.md)).
- Concurrent Quick notes sync on the same listener. Every sync request is answered only after the peer is authenticated as an enrolled device of the vault it names; the coordinator stores and serves operations and accepts a new writer only when its certificate names the authenticated device. Requests carry the vault and device identifiers, version vectors, operation hashes, and signed operations that contain Quick notes and tag values with the writer certificates; no other vault data travels this way. Operations are signed but not separately encrypted, so the mutual TLS link is their only confidentiality protection.
- Contact requests and status polls on the same LAN listener, which are the only inbound messages accepted without a client certificate (see [Contacts and contact requests](../sync.md#contacts-and-contact-requests)). The surface is bounded: a card is at most 512 bytes and must carry a valid signature before anything is written, the recipient card nonce is compared in constant time, status polls are signed and rejected outside a five-minute window, pending received requests are capped at 64 with the oldest expired first, and a blocked requester receives the same response as an accepted one. Outbound, the requester connects only to the endpoint and certificate fingerprint named in the card it was handed.
- The Chrome extension's native-messaging connection, which is local and not a remote service.

Each adapter owns its allowed schemes, origins, redirects, response bounds, timeouts, and credential handling. There is no generic command that fetches an arbitrary URL with application credentials.

Provider network behavior follows the selected provider and its own policy. Ganbaru AI constrains the process, workspace, environment, host tools, and context it supplies, but cannot promise that an external provider does not retain submitted content. The UI must make provider choice and data transfer understandable.

## Planned network surfaces

Not implemented:

- Concurrent sync of domains other than Quick notes, LAN peer exchange between enrolled devices, operation encryption, and optional remote synchronization through a user-hosted Rust relay or always-on anchor.
- A hosted or local BYOK chat widget separate from coding-agent Chat.
- A separately authorized external MCP service.
- A `ganbaru-ai` CLI with external integration commands.

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

The vault and SQLite database are not application-encrypted. Ganbaru AI relies on operating-system account isolation and full-disk encryption such as LUKS, BitLocker, or FileVault.

Why: this keeps user files accessible to ordinary backup and editing tools and avoids a second fragile key-recovery system. The cost is that malware running as the same user can read the vault, and documentation must state that limitation honestly.

Secrets are different. Provider credentials live behind operating-system credential references and are materialized only at the native operation that needs them. They never belong in `config.json`, the vault database, provider DTOs, diagnostics, or exports.

Sync writer keys are per installation and per vault. Desktop stores them in the operating-system keyring; Android stores them in private application files, with the same protection as the person key, until Keystore wrapping is implemented. Neither location is inside the vault, so writer keys never enter snapshots, backups, or device transfers. A restored or copied database is detected and gets a new writer instead of reusing an old key's sequence.

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
