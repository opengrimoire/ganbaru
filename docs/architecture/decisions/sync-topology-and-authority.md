# Sync topology and authority decisions

**Status: Implemented** for authority and LAN peer exchange. Recorded 2026-10-09, after Quick notes replication was implemented in source through a single coordinator hub, to remove that single point. The anchored authorization log, device and admin keys, removal cutoffs, late changes, fork evidence, quarantine, anchor handover (planned, forced by the recovery kit, and contested), the recovery kit, and LAN peer exchange between members are implemented in source. The relay and the always-on anchor are planned. The engine itself is explained by [Sync engine decisions](sync-engine.md), the contract by [Device linking and synchronization](../../data/sync.md), and people, spaces, and roles by [Contacts and invitations](../../features/collaboration/README.md).

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
| Leaderless content and delivery, one anchor per space for authority | The anchor is a member desktop that sequences a hash-chained authorization log of enrollments, removals, roles, admin keys, and handovers. Membership changes are rare, so ordering them in one place costs little, while leaderless authority has no satisfying answer for concurrent removals. |
| The anchor can move; the coordinator stays | Pairing and first snapshot copies stay with the coordinator, while each device pins the anchor separately and follows handovers through its peer book, so moving authority needs no relinking. |
| The anchor never gates content | Editing and delivery continue while the anchor is unreachable; only authorization proposals wait. |
| Log entries travel as content-addressed records | Every device stores entries in a record store keyed by hash and selects one chain deterministically (kit handovers, epoch, epoch root, revision, hash), so any path can carry them and a contest freezes at the common ancestor instead of splitting replicas. |
| Writer certificates are signed by the device key | Each installation's device key is what the log enrolls and removes, so certificates follow enrollment instead of the person key that every device holds. |
| A removal's cutoff is the anchor's stored vector | Operations the anchor has stored are final and can never be excluded. Operations only peers had seen stay provisional, and a removal may exclude them. A new anchor may raise a cutoff to what the coordinator reported storing, but only a bounded distance past what the anchor stored, so a lying report cannot keep a removed device's later operations. |
| Excluded operations become late changes | A removed device's provisional values are neither applied nor dropped silently; rows it created stay hidden, and an admin restores them as their own write or discards them. |
| A separate admin key signs authorization proposals | Every linked device holds the person key, so a stolen phone must not be able to enroll writers or remove the other devices. The admin key lives on the anchor and chosen admin devices, and the printed recovery kit derives the recovery admin key. |
| Device-local quarantine without the anchor | Any device can immediately refuse another while the anchor is unreachable. Quarantine never excludes operations by itself, so replicas still converge once the removal is sequenced. |
| Anchor handover is admin-signed and increments an anchor epoch | A lost, replaced, or restored anchor can hand over without consensus. Two concurrent takes freeze the epoch until one contestant yields or the kit decides, and a forked anchor freezes its epoch until a handover. |
| Every member installation serves its LAN peers | With removal decided by the anchor's log, any enrolled device can store and serve operations, authenticated by mutual TLS with the certificates its enrollment names. A device removed before the handshake still receives authorization records, so it learns of its removal. |
| A relay is blind and never an anchor (planned) | It cannot read content or validate authorization, so it only stores and forwards encrypted records. |
| An always-on anchor is an enrolled member, not a service (planned) | One self-hosted binary can run as a full member that holds the keys, which makes it the anchor, a hub reachable from every network, and a backup. This is the organization configuration and also suits one person with a home server. |
| Personal data is a space with one member | Devices and people use the same log, cutoff, and key rules; a person's anchor defaults to their coordinator desktop. |

## Rejected alternatives

- **Leaderless authority:** concurrent removals, two admins removing each other, and back-dated operations are open problems in research designs, and a deployed leaderless resolver needed a new algorithm and room version to stop state resets.
- **The anchor as a write gate:** makes every edit depend on one device being reachable, which breaks offline-first.
- **A single hub forever:** a switched-off coordinator would stop all sync, and every device would need to share its LAN.
- **Any hub sealing removals:** two hubs with different stored sequences would give different cutoffs for the same removal.
- **Project-operated relays or anchors:** conflict with the project's self-hosting principle and would centralize authority.
- **A shared folder as transport:** dropped; member listeners on the LAN and the planned relay cover the reachability it would add.
- **Vouching for operations past a cutoff:** accepting evidence from peers would let a removed device influence outcomes through peers that have not yet seen the removal. Cutoffs stay fixed, and an admin restoring late changes covers the honest case.

## Accepted limitations

- Membership and role changes wait while the anchor is unreachable. Quarantine covers urgent cases locally.
- Provisional state grows while the anchor is unreachable: compaction keeps the versions an exclusion could restore until the anchor confirms storing the operations that displaced them, and the record store never shrinks.
- Two contestants can freeze the authority log until one yields or the recovery kit decides; content keeps syncing meanwhile.
- A removal cannot erase copies a removed device already holds, as with any system that allows offline access.
- Content only a removed device held, and that nobody else received, is lost with the device.
- An organization that wants authority changes without its admins online must run an always-on anchor.

## Prior work

- [Keyhive lab notebook](https://www.inkandswitch.com/keyhive/notebook/01/): concurrent revocation and admins revoking each other are open problems, and consensus is treated as a last resort. A single anchor per space is that consensus, accepted deliberately because membership changes are rare.
- [Matrix Project Hydra](https://matrix.org/blog/2025/08/project-hydra-improving-state-res): leaderless state resolution allowed state resets (CVE-2025-49090), fixed in State Resolution v2.1 and room version 12.
- [any-sync](https://pkg.go.dev/github.com/anyproto/any-sync): local-first encrypted spaces whose self-hostable nodes include a consensus node for access control changes, a similar split between content and authority.
- [Joplin synchronization](https://joplinapp.org/help/apps/sync/): end-to-end encrypted sync to storage-only targets such as WebDAV, S3, and the file system, a precedent for blind delivery.
