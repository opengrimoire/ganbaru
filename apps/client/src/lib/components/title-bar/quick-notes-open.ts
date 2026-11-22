/** Which title bar surfaces that replace the Quick notes panel are open. */
export interface TitleBarSurfaces {
  readonly music: boolean;
  readonly pomodoro: boolean;
  readonly performance: boolean;
  readonly titleBarMenu: boolean;
  readonly tabContextMenu: boolean;
  readonly utilityOverflow: boolean;
  readonly themeQuickSwitcher: boolean;
  readonly settings: boolean;
}

/** A Quick notes open waiting for its preload, named by a token and remembering the surfaces open when it started. */
export interface QuickNotesOpenRequest {
  readonly token: symbol;
  readonly surfacesAtStart: TitleBarSurfaces;
}

/**
 * Starts a Quick notes open. Its token supersedes any open still waiting.
 *
 * @param surfaces The surfaces open now.
 */
export function startQuickNotesOpen(surfaces: TitleBarSurfaces): QuickNotesOpenRequest {
  return { token: Symbol("quick notes open"), surfacesAtStart: surfaces };
}

/**
 * Whether a Quick notes open whose preload just finished still shows the panel. It does not when a later toggle
 * cancelled or replaced it, or when the user opened another surface while it waited.
 *
 * @param pending The open still waiting, or null when none is.
 * @param request The open whose preload finished.
 * @param surfaces The surfaces open now.
 */
export function quickNotesOpenApplies(
  pending: QuickNotesOpenRequest | null,
  request: QuickNotesOpenRequest,
  surfaces: TitleBarSurfaces,
): boolean {
  if (pending?.token !== request.token) return false;
  const keys = Object.keys(surfaces) as (keyof TitleBarSurfaces)[];
  return keys.every((key) => !surfaces[key] || request.surfacesAtStart[key]);
}
