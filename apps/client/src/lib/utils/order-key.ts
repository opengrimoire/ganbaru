/**
 * Compares two strings by UTF-16 code unit, never by locale. Order keys and generated IDs are ASCII, so this matches
 * the byte order SQLite and Rust use for `(order_key, id)` lists.
 */
export function compareCodeUnits(left: string, right: string): number {
  if (left === right) return 0;
  return left < right ? -1 : 1;
}

/** Orders rows like their native lists: by order key, then by id as the tiebreaker. */
export function compareOrderKeyThenId(leftKey: string, leftId: string, rightKey: string, rightId: string): number {
  return compareCodeUnits(leftKey, rightKey) || compareCodeUnits(leftId, rightId);
}
