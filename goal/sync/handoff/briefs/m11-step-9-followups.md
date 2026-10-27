Step 8 review follow-ups (low) for milestone 11 step 9:
1. The 30-day anchor suggestion (`authority.ts` ~294, `en/sync.ts` ~79) measures the age of the oldest unconfirmed change but says "the anchor has been unreachable", shows on members and Android, and cannot be acted on until forced handover exists. Show it only on admin desktops, word it as what is measured, and add the plan's unreachable duration and displaced-version count when step 9 has them.
2. Restoring a late change from an admin that is not the anchor may count the anchor's republished value as a newer edit, because those tags are anchor-local (known since step 5). Handle or document.
3. The late change list has no UI bound (the outbox is capped at 256). Bound it like the outbox.
