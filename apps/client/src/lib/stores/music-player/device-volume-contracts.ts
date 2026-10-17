/** Platform media volume that replaces the in-app Music volume where the platform expects it. */
export interface DeviceVolume {
  /** Current level from 0 to 1, or null when the app volume applies instead. */
  read(): number | null;
  /** Sets the nearest device level and returns the level that applies, or null when the app volume applies. */
  set(level: number): number | null;
  /** Calls the listener after the device volume may have changed, and returns the unsubscribe function. */
  subscribe(listener: () => void): () => void;
}
