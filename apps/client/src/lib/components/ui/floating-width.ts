/**
 * Preferred floating surface widths in pixels, matching the `w-floating-sm`, `w-floating`, and `w-floating-lg` utilities.
 * Positioning code that measures or clamps a panel uses these; static markup uses the utilities.
 */
export const FLOATING_WIDTH = {
  /** Compact action menus, such as row, column, and context menus. */
  sm: 240,
  /** Settings and option panels with labeled controls. */
  md: 280,
  /** Lists with search, editors, and pickers with two-part rows. */
  lg: 320,
} as const;
