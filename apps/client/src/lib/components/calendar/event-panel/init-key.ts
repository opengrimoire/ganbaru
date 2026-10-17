export type EventPanelInitMode = "create" | "edit";

export interface EventPanelInitKeyInput {
  parked: boolean;
  mode: EventPanelInitMode;
  panelSessionKey: number;
  eventId?: string;
}

/**
 * Builds the identity used for one full EventPanel initialization.
 * Editable draft fields such as start, end, and all-day are intentionally
 * excluded so changing them does not reset title, color, meeting details, or
 * other in-progress create-panel data.
 */
export function buildEventPanelInitKey({
  parked,
  mode,
  panelSessionKey,
  eventId,
}: EventPanelInitKeyInput): string {
  const surface = parked ? "parked" : "active";
  if (mode === "edit") return `${surface}:edit:${panelSessionKey}:${eventId ?? ""}`;
  return `${surface}:create:${panelSessionKey}`;
}
