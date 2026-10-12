# Sync topology and authority decisions

**Status: Reference.** Recorded 2026-10-09, after Quick notes replication was implemented in source through a single coordinator hub. This record sets the planned direction for removing that single point; none of it is implemented yet. The engine itself is explained by [Sync engine decisions](sync-engine.md), the contract by [Device linking and synchronization](../../data/sync.md), and people, spaces, and roles by [Contacts and invitations](../../features/collaboration/README.md).

## Constraints

- Any linked device that is up to date should be able to update the others, over any network that connects them. A coordinator that is switched off must not stop two devices on the same LAN from syncing.
- Removing a device, and later a person, must stop its future operations everywhere and decide deterministically which of its operations count, even when the removal is concurrent with offline edits.
- A blackout, outage, or unreachable server must never stop anyone from editing. Only membership and role changes may wait.
- No project-operated infrastructure. Anything always-on is user-provisioned and self-hostable.
- Multi-person sharing and organizations are configurations of the same design, not a second system.

## Decisions

| Decision | Reason |
| --- | --- |
| Treat content merging, delivery, and authority as separate concerns | Content already merges without a leader, and every operation is signed and validated by its receiver, so any path can carry it. Only authority needs ordering. |
| Leaderless content and delivery, one anchor per space for authority | The anchor is a member installation that holds the space keys and sequences a hash-chained authorization log of enrollments, removals, roles, admin keys, and handovers. Membership changes are rare, so ordering them in one place costs little, while leaderless authority has no satisfying answer for concurrent removals. |
| The anchor never gates content | Editing and delivery continue while the anchor is unreachable; only authorization proposals wait. |
| A removal's cutoff is the anchor's stored vector | Operations the anchor has stored are final and can never be excluded. Operations only peers had seen stay provisional, and a removal may exclude them. This generalizes the current hub cutoff and keeps every replica's result deterministic. |
| Excluded operations become late changes | A removed device's provisional values are neither applied nor dropped silently; an admin restores them as their own write or discards them. |
| A separate admin key signs authorization proposals | Every linked device holds the person key, so a stolen phone must not be able to enroll writers or remove the other devices. The admin key lives on the anchor and chosen admin devices, with a backup in the recovery kit. |
| Device-local quarantine without the anchor | Any device can immediately refuse another while the anchor is unreachable. Quarantine never excludes operations by itself, so replicas still converge once the removal is sequenced. |
| Anchor handover is admin-signed and increments an anchor epoch | A lost, replaced, or always-on anchor can take over without consensus. Entries a stale anchor sequenced after the handover return to proposals, and a restored or cloned anchor is detected the same way writers are. |
| Every member installation serves its LAN peers | With removal decided by the anchor's log, any enrolled device can store and serve operations, authenticated by mutual TLS with the certificates its enrollment names. |
| A relay is blind and never an anchor | It cannot read content or validate authorization, so it only stores and forwards encrypted records. |
| An always-on anchor is an enrolled member, not a service | One self-hosted binary can run as a full member that holds the keys, which makes it the anchor, a hub reachable from every network, and a backup. This is the organization configuration and also suits one person with a home server. |
| Personal data is a space with one member | Devices and people use the same log, cutoff, and key rules; a person's anchor defaults to their coordinator desktop. |

## Rejected alternatives

- **Leaderless authority:** concurrent removals, two admins removing each other, and back-dated operations are open problems in research designs, and a deployed leaderless resolver needed a new algorithm and room version to stop state resets.
- **The anchor as a write gate:** makes every edit depend on one device being reachable, which breaks offline-first.
- **A single hub forever:** a switched-off coordinator would stop all sync, and every device would need to share its LAN.
- **Any hub sealing removals:** two hubs with different stored sequences would give different cutoffs for the same removal.
- **Project-operated relays or anchors:** conflict with the project's self-hosting principle and would centralize authority.

## Accepted limitations

- Membership and role changes wait while the anchor is unreachable. Quarantine covers urgent cases locally.
- Provisional state grows while the anchor is unreachable, so compaction must keep register state for provisional versions until the anchor stores them.
- A removal cannot erase copies a removed device already holds, as with any system that allows offline access.
- Content only a removed device held, and that nobody else received, is lost with the device.
- An organization that wants authority changes without its admins online must run an always-on anchor.

## Prior work

- [Keyhive lab notebook](https://www.inkandswitch.com/keyhive/notebook/01/): concurrent revocation and admins revoking each other are open problems, and consensus is treated as a last resort. A single anchor per space is that consensus, accepted deliberately because membership changes are rare.
- [Matrix Project Hydra](https://matrix.org/blog/2025/08/project-hydra-improving-state-res): leaderless state resolution allowed state resets (CVE-2025-49090), fixed in State Resolution v2.1 and room version 12.
- [any-sync](https://pkg.go.dev/github.com/anyproto/any-sync): local-first encrypted spaces whose self-hostable nodes include a consensus node for access control changes, a similar split between content and authority.
- [Joplin synchronization](https://joplinapp.org/help/apps/sync/): end-to-end encrypted sync to storage-only targets such as WebDAV, S3, and the file system, a precedent for blind delivery.
