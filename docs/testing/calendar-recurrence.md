# Calendar recurrence testing

**Status: Reference.** Recurrence testing must prove that the visible preview, the committed plan, persisted rows, canonical expansion, protected Focus history, and active Pomodoro references agree. Editing behavior is specified in [Recurrence editing](../features/calendar/recurrence-editing.md).

## Automated coverage

Canonical Rust expansion is the only production recurrence engine, so recurrence correctness is tested natively:

- **Expansion and lookup:** COUNT before exclusions, UNTIL forms, independent RDATE, advanced selectors, home-zone identities across UTC midnight, DST gaps and folds, floating all-day dates, and explicit work-budget exhaustion.
- **Partitioning:** each scoped split is serialized, reloaded through canonical expansion, and its disjoint union must equal the original dates and instants.
- **Preview and commit parity:** the complete visible preview is compared with the committed plan reloaded through the canonical window reader, including metadata, overrides, and occurrence identities.
- **Protection:** started, tracked, overridden, and active occurrences keep their identity and history; Calendar writes, Focus reference changes, and receipts commit or roll back together.
- **Metadata:** imported envelopes, unknown parameters, attendees, and alarms survive detach, split, and copy, within shared size limits.
- **Deletion and Undo:** archives round-trip complete preimages, Undo restores the prior structure and rejects changed sources, and hard-deleted future sources keep their preimage only in process memory.
- **Receipts:** retries after a lost reply return the original outcome without new writes, reject changed intent, and stay bound to the original vault.

Frontend tests cover draft serialization, coalesced preview requests, stale reply suppression, retry of exact requests after lost replies, and cache invalidation across windows, with mocked native boundaries.

## Edit matrix

Test each row for the template's first occurrence and for a generated occurrence where both are valid.

| Draft operation | Scope | Required result |
| --- | --- | --- |
| Non-recurring event gains repeat | No scope selector | Existing row becomes the first template occurrence and keeps its identity. |
| Fields change, recurrence unchanged | Only this | Selected occurrence detaches; source series continues with one exception. |
| Recurrence changes | Only this | Selected occurrence becomes an independent recurring template. |
| Fields or recurrence change | Following | Old template caps before selection and new template starts at selection. |
| Repeat clears | Following | Old template caps and one standalone survivor remains. |
| Recurrence changes, no protected history | All | Template may update directly. |
| Recurrence changes with protected history | All | Historical template remains unchanged through the boundary; mutable template carries the new rule. |
| Repeat clears with protected history | All | Protected history remains and selected mutable occurrence becomes the sole survivor. |

## Protection scenarios

- Adding an exception for started, tracked, overridden, active, and future untracked occurrences.
- Moving an end date or reducing COUNT before protected occurrences.
- Changing a rule so protected dates no longer match.
- Time shifts that would otherwise disconnect event geometry from recorded segments.
- Same-day occurrences on both sides of the captured edit time.
- Exceptions that must transfer across a following split.
- Unsupported or malformed imported rules that cannot be enumerated safely.

## Active-session scenarios

- Adding recurrence to an active non-recurring event retains the base identity.
- Editing the selected active occurrence forces `Only this` and protects its start.
- A following edit across a later active occurrence materializes it unchanged before splitting.
- An all edit from another occurrence leaves the active protected occurrence unchanged.
- End now and Enable Focus commit Calendar and Focus changes in one transaction and do not duplicate execution on retry.
- Run and segment transfer rolls back with the Calendar transaction on failure.

## Delete and archive scenarios

- Only the selected future untracked occurrence hard deletes.
- Protected selected occurrences archive.
- Following from a started occurrence affects started history without deleting later mutable occurrences.
- Following from a future occurrence preserves protected history and removes the mutable future chain.
- All on a future-only untracked series can delete the template.
- All with protected history preserves the historical side and removes only the intended mutable side.
- Undo restores the complete prior recurrence structure and does not restart Pomodoro history.

## Manual acceptance

Run on desktop and Android with representative daily, weekly, monthly, and advanced rules:

1. Open an occurrence and change scope without editing fields. Only the affected contour changes.
2. Edit fields, clear and restore repeat, and move between scopes. The same draft is retained.
3. Save and confirm the immediate view matches a restart and a fresh window load.
4. Repeat with started history and an active occurrence elsewhere in the series.
5. Create, drag a recurring draft, and interrupt the response before Save completes. Retry produces one result.
6. Schedule a Project task batch with custom Focus settings, interrupt the response, retry, and confirm the same events and task history after restart.
7. Delete with each scope, then use Undo within its toast window, including across desktop sleep and Android backgrounding.
8. Trigger a persistence failure and confirm no partial structure or stale preview remains.
