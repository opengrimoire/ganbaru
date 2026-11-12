import type { ChatTurnId } from "$lib/chat/contracts";

import type { TimelineRow } from "./types";

/**
 * Groups projected rows by their turn in one pass, keeping input order within each turn.
 *
 * @param rows Projected rows in any order.
 * @returns Rows of each turn; rows without a turn are left out.
 */
export function indexTimelineRowsByTurn(rows: Iterable<TimelineRow>): Map<ChatTurnId, TimelineRow[]> {
  const rowsByTurn = new Map<ChatTurnId, TimelineRow[]>();
  for (const row of rows) {
    if (!row.turnId) continue;
    const turnRows = rowsByTurn.get(row.turnId);
    if (turnRows) turnRows.push(row);
    else rowsByTurn.set(row.turnId, [row]);
  }
  return rowsByTurn;
}
