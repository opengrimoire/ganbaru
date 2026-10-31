/** Stored thousandths per whole playback multiplier. The vault stores volumes, rates, and levels this way. */
export const THOUSANDTHS_PER_UNIT = 1000;

/** Stored integer thousandths as the multiplier that audio output applies. */
export function fromThousandths(value: number): number {
  return value / THOUSANDTHS_PER_UNIT;
}

/** A multiplier as the nearest whole thousandths the vault stores. */
export function toThousandths(value: number): number {
  return Math.round(value * THOUSANDTHS_PER_UNIT);
}
