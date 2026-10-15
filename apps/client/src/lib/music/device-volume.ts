/**
 * Parses the Android media volume state reported by the app shell.
 *
 * @param state Volume as `index,max`, where the device has `max` steps; empty when apps cannot change it.
 * @returns The volume level from 0 to 1, or null when the state is empty or malformed.
 */
export function parseDeviceVolumeState(state: string): number | null {
  const match = /^(\d+),(\d+)$/.exec(state.trim());
  if (!match) return null;
  const index = Number(match[1]);
  const max = Number(match[2]);
  if (max <= 0) return null;
  return Math.min(1, index / max);
}
