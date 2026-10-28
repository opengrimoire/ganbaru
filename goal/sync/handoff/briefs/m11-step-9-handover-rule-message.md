Handover selection rule for Critical 7 (decided by the orchestrator; implement after the items you already have, in its own commits, with the tests below; record the decision and the residual risks in the progress file and in `docs/data/sync.md`).

Trust order at a fork point P (the entry competing branches continue from): a handover signed by a recovery key of kit lineage > the anchor's continuation of its own epoch > a device-admin handover.

Kit lineage: the charter's recovery key, and any recovery key installed by a `SetRole::Recovery` signed by the then current kit-lineage recovery key. `SetRole::Recovery` signed by a device admin stays allowed (regenerating a lost kit keeps working), but the key it installs is not of kit lineage: it is a full admin key for proposals and forced handovers, and its handovers rank and can be vetoed like a device-admin handover. Track lineage in `AuthState` (for example a flag next to `recovery_admin_key`).

Selection (order independent, a pure function of stored check-valid entries; keep it inside `select`):
```
tree = check-valid nodes from the charter (as today)
reachable = max over ALL check-valid chains, vetoed ones included   // readiness unchanged
post-order, for each node n:
  ev(n) = revocations in n itself ∪ ev(c) for every child c of n that is not vetoed
  // revocations: removed device ids, demoted device ids, revoked or replaced admin keys,
  // and rotations away from kit-lineage key k signed by k
for each handover H whose parent is P (author key a, target T):
  E = ev(the non-handover children of P)   // only the continuation's subtree, so sibling handovers never veto each other
  vetoed(H) = a is a kit-lineage recovery key at P
                ? E has a rotation away from a signed by a
                : E revokes a, or removes or demotes T, or removes or demotes the device holding a
skip vetoed subtrees
rank(head) = (number of kit-lineage recovery handovers on the path, epoch,
              Reverse(epoch_root), revision, Reverse(head hash))
```
Entries past a freeze already fail `check`, so they are never evidence. The restored anchor's self-handover from its stale head stays valid (the anchor never removes itself); keep item b of the brief consistent with this path.

Supporting changes:
- The kit forced-handover path must be able to continue from the highest stored revision where its target is still live, not only the selected head (`sequence` Handover branch), and seals `remove(old anchor)` right after the handover and the `set role` admin for itself.
- Store: kind 4 authorization entries need their own bound separate from the excluded operation cap (cutoff + 1,024), and writer genesis admission must accept an enrollment on any check-valid stored chain, not only the selected one, so a replica that first selected an attacker branch still stores the honest veto evidence and converges. Keep finding 10's bound on excluded domain operations.

Tests (each in both arrival orders, plus one simulation over random arrival orders): stale admin removed or demoted chains self-handovers to epoch e+5 and is vetoed; same with the demotion in a later epoch after a planned handover; handover to a removed target vetoed; stolen anchor: its admin-signed rotation does not veto a kit handover, it vetoes an admin-forced handover, it does not veto the kit handover after removing the kit's target, its self-handover chain from below the kit's fork point loses on recovery count, its handover inside the kit epoch is vetoed once its removal is stored, and a kit it installed by admin-signed rotation carries no precedence; forced handover after loss wins over unseen continuation entries and is vetoed when they remove the forcer; restored anchor self-handover beats its own lost continuation; leaked old kit after a kit-signed rotation is vetoed from before the rotation and refused (`NotAdmin`) after it; sibling admin handovers do not veto each other; clone entries past the freeze are not evidence; `reachable` unchanged when the longest chain is vetoed; honest kind 4 evidence past cutoff + 1,024 is stored. Unignore the stale admin test from item c.

Docs (a few sentences, current behavior): `docs/data/sync.md` topology and authority bullets (trust order at a fork, kit lineage, regenerating without the current code gives a kit that works as an admin key but cannot overrule a stolen anchor; regenerate with the current code to revoke a leaked one), and the residual risks in the security summary of `docs/data/security/network-and-privacy.md`: kit compromise is total; a stolen anchor with a lost or admin-regenerated kit needs re-founding (Reset sync state and relink); a stolen admin desktop not yet removed can force a handover until its removal is stored; a leaked old kit wins on replicas that have not stored the rotation yet.
