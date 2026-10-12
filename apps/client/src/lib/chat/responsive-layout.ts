export type ChatLayoutVariant =
  | "three_column"
  | "no_inspector"
  | "rail_sheet"
  | "inspector_sheet"
  | "minimum_recovery";

export interface ChatLayoutInput {
  containerWidth: number;
  containerHeight: number;
  fontScale: number;
  railOpen: boolean;
  inspectorOpen: boolean;
  inspectorWidth: number;
  replyThreadOpen?: boolean;
  replyThreadWidth?: number;
  previousVariant?: ChatLayoutVariant;
}

export interface ChatLayoutDecision {
  variant: ChatLayoutVariant;
  railPresentation: "column" | "sheet";
  inspectorPresentation: "closed" | "column" | "sheet";
  replyThreadPresentation: "closed" | "column" | "main";
  activeSurface: "conversation" | "rail" | "inspector";
}

export const CHAT_CONVERSATION_MIN_PX = 440;
export const CHAT_REPLY_THREAD_WIDTH_PX = 440;
export const CHAT_AUXILIARY_CONVERSATION_MIN_PX = 320;
export const CHAT_RAIL_WIDTH_PX = 256;
export const CHAT_RAIL_COLLAPSED_WIDTH_PX = 44;
const BASE_MINIMUM_ENTER_WIDTH = 320;
const BASE_MINIMUM_EXIT_WIDTH = 360;
const BASE_MINIMUM_ENTER_HEIGHT = 210;
const BASE_MINIMUM_EXIT_HEIGHT = 240;
const BASE_HYSTERESIS = 24;
const INSPECTOR_FIT_RATIO = 0.38;
const INSPECTOR_FIT_MINIMUM = 420;
const INSPECTOR_FIT_MAXIMUM = 720;
const BOTTOM_PANEL_FIT_RATIO = 0.32;
const BOTTOM_PANEL_FIT_MINIMUM = 190;
const BOTTOM_PANEL_FIT_MAXIMUM = 360;
const BOTTOM_PANEL_MAXIMUM_RATIO = 0.5;
const BASE_CONVERSATION_HEIGHT_MIN = 240;
const COLLAPSE_SNAP_RATIO = 0.45;

export interface ChatInspectorResizeInput {
  containerWidth: number;
  railVisible: boolean;
  fontScale?: number;
  minimum: number;
  maximum: number;
  railReservedWidth?: number;
  conversationMinimum?: number;
  additionalReservedWidth?: number;
}

export interface ChatBottomPanelResizeInput {
  containerHeight: number;
  fontScale?: number;
  minimum: number;
  maximum: number;
}

/**
 * Bounds a column inspector while reserving the conversation reading width.
 *
 * @param input Current shell, rail, and inspector constraints.
 * @returns The largest inspector width that keeps the conversation usable.
 */
export function chatInspectorResizeMaximum(input: ChatInspectorResizeInput): number {
  const occupiedByRail = input.railReservedWidth
    ?? (input.railVisible ? CHAT_RAIL_WIDTH_PX : CHAT_RAIL_COLLAPSED_WIDTH_PX);
  const available = input.containerWidth
    - occupiedByRail
    - (input.conversationMinimum ?? scaledConversationWidth(input.fontScale))
    - (input.additionalReservedWidth ?? 0);
  return clampPanelSizeToWholePixel(
    available,
    input.minimum,
    Math.min(input.maximum, available),
  );
}

/**
 * Selects a comfortable inspector width for code, files, and plans.
 *
 * @param input Current shell and rail constraints.
 * @returns A context-sensitive inspector width inside the hard resize bounds.
 */
export function fittedChatInspectorWidth(input: ChatInspectorResizeInput): number {
  const scale = boundedFontScale(input.fontScale);
  return fittedPanelSize(
    input.containerWidth * INSPECTOR_FIT_RATIO,
    INSPECTOR_FIT_MINIMUM * scale,
    INSPECTOR_FIT_MAXIMUM * scale,
    chatInspectorResizeMaximum(input),
  );
}

/**
 * Bounds the bottom panel while retaining a useful conversation height.
 *
 * @param input Current shell height and panel constraints.
 * @returns The largest useful bottom panel height for the current layout.
 */
export function chatBottomPanelResizeMaximum(input: ChatBottomPanelResizeInput): number {
  const scale = boundedFontScale(input.fontScale);
  const ratioMaximum = Math.floor(input.containerHeight * BOTTOM_PANEL_MAXIMUM_RATIO);
  const conversationMaximum = Math.floor(
    input.containerHeight - BASE_CONVERSATION_HEIGHT_MIN * scale,
  );
  return clampPanelSizeToWholePixel(
    Math.min(input.maximum, ratioMaximum, conversationMaximum),
    input.minimum,
    input.maximum,
  );
}

/**
 * Selects a comfortable bottom panel height for terminals and diffs.
 *
 * @param input Current shell height and panel constraints.
 * @returns A context-sensitive height inside the hard resize bounds.
 */
export function fittedChatBottomPanelHeight(input: ChatBottomPanelResizeInput): number {
  const scale = boundedFontScale(input.fontScale);
  return fittedPanelSize(
    input.containerHeight * BOTTOM_PANEL_FIT_RATIO,
    BOTTOM_PANEL_FIT_MINIMUM * scale,
    BOTTOM_PANEL_FIT_MAXIMUM * scale,
    chatBottomPanelResizeMaximum(input),
  );
}

/**
 * Snaps a dragged panel into an explicit collapsed state near its closed edge.
 *
 * @param requested Raw pointer-derived size.
 * @param minimum Smallest usable open size.
 * @param maximum Largest allowed open size.
 * @returns Zero for the collapse zone, otherwise a bounded open size.
 */
export function panelSizeWithCollapseSnap(
  requested: number,
  minimum: number,
  maximum: number,
): number {
  if (requested <= minimum * COLLAPSE_SNAP_RATIO) return 0;
  return clampPanelSizeToWholePixel(requested, minimum, maximum);
}

/**
 * Selects the Chat shell layout from measured fit while preserving the prior
 * layout inside a hysteresis band.
 *
 * @param input Measured shell and panel constraints.
 * @returns Stable panel presentations and the primary visible surface.
 */
export function chatLayoutDecision(input: ChatLayoutInput): ChatLayoutDecision {
  const scale = Math.max(1, Math.min(2, input.fontScale));
  const previous = input.previousVariant;
  const replyThreadOpen = input.replyThreadOpen ?? false;
  const replyThreadWidth = input.replyThreadWidth ?? CHAT_REPLY_THREAD_WIDTH_PX * scale;
  const needsMinimumRecovery = previous === "minimum_recovery"
    ? input.containerWidth < BASE_MINIMUM_EXIT_WIDTH * scale
      || input.containerHeight < BASE_MINIMUM_EXIT_HEIGHT * scale
    : input.containerWidth < BASE_MINIMUM_ENTER_WIDTH * scale
      || input.containerHeight < BASE_MINIMUM_ENTER_HEIGHT * scale;
  if (needsMinimumRecovery) {
    return {
      variant: "minimum_recovery",
      railPresentation: "sheet",
      inspectorPresentation: input.inspectorOpen ? "sheet" : "closed",
      replyThreadPresentation: replyThreadOpen ? "main" : "closed",
      activeSurface: input.inspectorOpen ? "inspector" : input.railOpen ? "rail" : "conversation",
    };
  }

  if (replyThreadOpen) {
    const conversationMin = CHAT_AUXILIARY_CONVERSATION_MIN_PX * scale;
    const threadColumnRequired = conversationMin
      + replyThreadWidth
      + (input.inspectorOpen ? input.inspectorWidth : 0);
    const replyThreadPresentation = input.containerWidth >= threadColumnRequired
      ? "column"
      : "main";
    const contentRequired = replyThreadPresentation === "column"
      ? conversationMin + replyThreadWidth
      : CHAT_CONVERSATION_MIN_PX * scale;
    const inspectorFits = input.inspectorOpen
      && input.containerWidth >= contentRequired + input.inspectorWidth;
    const inspectorPresentation = !input.inspectorOpen
      ? "closed"
      : inspectorFits ? "column" : "sheet";
    const railWidth = input.railOpen ? CHAT_RAIL_WIDTH_PX : CHAT_RAIL_COLLAPSED_WIDTH_PX;
    const railFits = !input.inspectorOpen
      && input.containerWidth >= contentRequired + railWidth;
    const railPresentation = input.inspectorOpen || !railFits ? "sheet" : "column";
    return {
      variant: inspectorPresentation === "column"
        ? "three_column"
        : inspectorPresentation === "sheet"
          ? "inspector_sheet"
          : railPresentation === "sheet" ? "rail_sheet" : "no_inspector",
      railPresentation,
      inspectorPresentation,
      replyThreadPresentation,
      activeSurface: inspectorPresentation === "sheet" ? "inspector" : "conversation",
    };
  }

  const conversationMin = CHAT_CONVERSATION_MIN_PX * scale;
  const railWidth = input.railOpen ? CHAT_RAIL_WIDTH_PX : CHAT_RAIL_COLLAPSED_WIDTH_PX;
  const railRequired = railWidth + conversationMin;
  const railWasColumn = previous === "three_column"
    || previous === "no_inspector"
    || previous === "inspector_sheet";
  const railFits = stableFit(input.containerWidth, railRequired, railWasColumn, scale);
  if (!railFits) {
    return {
      variant: "rail_sheet",
      railPresentation: "sheet",
      inspectorPresentation: input.inspectorOpen ? "sheet" : "closed",
      replyThreadPresentation: "closed",
      activeSurface: input.inspectorOpen ? "inspector" : "conversation",
    };
  }

  if (!input.inspectorOpen) {
    return {
      variant: "no_inspector",
      railPresentation: "column",
      inspectorPresentation: "closed",
      replyThreadPresentation: "closed",
      activeSurface: "conversation",
    };
  }

  const inspectorRequired = railRequired + input.inspectorWidth;
  const inspectorFits = stableFit(
    input.containerWidth,
    inspectorRequired,
    previous === "three_column",
    scale,
  );
  return inspectorFits
    ? {
        variant: "three_column",
        railPresentation: "column",
        inspectorPresentation: "column",
        replyThreadPresentation: "closed",
        activeSurface: "conversation",
      }
    : {
        variant: "inspector_sheet",
        railPresentation: "column",
        inspectorPresentation: "sheet",
        replyThreadPresentation: "closed",
        activeSurface: "inspector",
      };
}

/**
 * Lists the recovery routes that every layout must keep available.
 *
 * @param decision Current layout decision.
 * @returns Stable primary action identifiers.
 */
export function chatLayoutPrimaryActions(
  decision: ChatLayoutDecision,
): readonly ["threads", "composer", "requests", "stop", "settings", "inspector"] {
  void decision;
  return ["threads", "composer", "requests", "stop", "settings", "inspector"];
}

export function chatLayoutsEqual(left: ChatLayoutDecision, right: ChatLayoutDecision): boolean {
  return left.variant === right.variant
    && left.railPresentation === right.railPresentation
    && left.inspectorPresentation === right.inspectorPresentation
    && left.replyThreadPresentation === right.replyThreadPresentation
    && left.activeSurface === right.activeSurface;
}

export interface PanelResizeInput {
  current: number;
  minimum: number;
  maximum: number;
  defaultValue: number;
  step: number;
  direction: "standard" | "reversed";
  key: string;
}

/**
 * Clamps a panel size to a whole CSS pixel inside whole-pixel bounds.
 *
 * @param value Requested panel size.
 * @param minimum Smallest permitted size.
 * @param maximum Largest permitted size.
 * @returns A bounded whole-pixel panel size.
 */
export function clampPanelSizeToWholePixel(
  value: number,
  minimum: number,
  maximum: number,
): number {
  const alignedMinimum = Math.ceil(minimum);
  const alignedMaximum = Math.max(alignedMinimum, Math.floor(maximum));
  return Math.max(alignedMinimum, Math.min(alignedMaximum, Math.round(value)));
}

export interface DevicePixelAlignedPanelSizeInput {
  value: number;
  minimum: number;
  maximum: number;
  anchor: number;
  direction: "from-start" | "from-end";
  devicePixelRatio: number;
}

/**
 * Aligns a panel boundary to the physical display pixel grid.
 *
 * @param input Requested size, bounds, fixed edge, and display scale.
 * @returns A bounded CSS size whose moving edge lands on a device pixel.
 */
export function alignPanelSizeToDevicePixel(
  input: DevicePixelAlignedPanelSizeInput,
): number {
  const scale = Number.isFinite(input.devicePixelRatio) && input.devicePixelRatio > 0
    ? input.devicePixelRatio
    : 1;
  const minimum = Math.min(input.minimum, input.maximum);
  const maximum = Math.max(input.minimum, input.maximum);
  const sign = input.direction === "from-start" ? 1 : -1;
  const requested = Math.max(minimum, Math.min(maximum, input.value));
  const requestedBoundary = input.anchor + requested * sign;
  let boundary = Math.round(requestedBoundary * scale) / scale;
  let size = (boundary - input.anchor) * sign;
  if (size < minimum) {
    boundary = input.direction === "from-start"
      ? Math.ceil((input.anchor + minimum) * scale) / scale
      : Math.floor((input.anchor - minimum) * scale) / scale;
    size = (boundary - input.anchor) * sign;
  } else if (size > maximum) {
    boundary = input.direction === "from-start"
      ? Math.floor((input.anchor + maximum) * scale) / scale
      : Math.ceil((input.anchor - maximum) * scale) / scale;
    size = (boundary - input.anchor) * sign;
  }
  return Math.max(minimum, Math.min(maximum, size));
}

/**
 * Resolves a keyboard separator action to a bounded panel width.
 *
 * @param input Current panel bounds, direction, and pressed key.
 * @returns The next width, or null when the key is not a resize action.
 */
export function panelWidthFromKey(input: PanelResizeInput): number | null {
  const sign = input.direction === "reversed" ? -1 : 1;
  let next: number;
  switch (input.key) {
    case "ArrowLeft": next = input.current - input.step * sign; break;
    case "ArrowRight": next = input.current + input.step * sign; break;
    case "Home": next = input.minimum; break;
    case "End": next = input.maximum; break;
    case "Enter": next = input.defaultValue; break;
    default: return null;
  }
  return clampPanelSizeToWholePixel(next, input.minimum, input.maximum);
}

/**
 * Selects scroll behavior without bypassing a reduced-motion preference.
 *
 * @param reducedMotion Whether the user requests reduced motion.
 * @returns Immediate or smooth scrolling behavior.
 */
export function chatScrollBehavior(reducedMotion: boolean): ScrollBehavior {
  return reducedMotion ? "auto" : "smooth";
}

/**
 * Preserves both ends of a long label for compact breadcrumbs and paths.
 *
 * @param value Full user-visible label.
 * @param maximumCodePoints Maximum displayed Unicode code points.
 * @returns The original label or a middle-truncated representation.
 */
export function middleTruncate(value: string, maximumCodePoints = 40): string {
  const points = Array.from(value);
  const maximum = Math.max(5, Math.floor(maximumCodePoints));
  if (points.length <= maximum) return value;
  const available = maximum - 1;
  const start = Math.ceil(available / 2);
  const end = Math.floor(available / 2);
  return `${points.slice(0, start).join("")}…${points.slice(points.length - end).join("")}`;
}

function boundedFontScale(fontScale = 1): number {
  return Math.max(1, Math.min(2, fontScale));
}

function scaledConversationWidth(fontScale = 1): number {
  return CHAT_CONVERSATION_MIN_PX * boundedFontScale(fontScale);
}

function fittedPanelSize(
  requested: number,
  preferredMinimum: number,
  preferredMaximum: number,
  hardMaximum: number,
): number {
  const maximum = Math.min(preferredMaximum, hardMaximum);
  const minimum = Math.min(preferredMinimum, maximum);
  return clampPanelSizeToWholePixel(requested, minimum, maximum);
}

function stableFit(
  available: number,
  required: number,
  previouslyFit: boolean,
  scale: number,
): boolean {
  const hysteresis = BASE_HYSTERESIS * scale;
  return previouslyFit
    ? available >= required - hysteresis
    : available >= required + hysteresis;
}
