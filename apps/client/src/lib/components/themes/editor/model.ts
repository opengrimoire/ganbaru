import type {
  CalendarColorDefaultMode,
  ThemeSources,
} from "$lib/themes";
import type { Translate } from "$lib/i18n/translator.svelte";

export type TokenInfo = { title: string; description: string };

export type GroupSingleRow = {
  kind: "single";
  key: string;
  scope: "app" | "cal";
};

export type GroupPairRow = {
  kind: "pair";
  bg: string;
  fg: string;
  title: string;
  description: string;
  scope: "app" | "cal";
  target?: number;
};

export type GroupSourcePairRow = {
  kind: "source-pair";
  bg: string;
  fg: string;
  bgSource: keyof ThemeSources;
  fgSource: keyof ThemeSources;
  title: string;
  description: string;
  scope: "app";
  target?: number;
};

export type GroupContrastRow = GroupPairRow | GroupSourcePairRow;
export type GroupRow = GroupSingleRow | GroupPairRow | GroupSourcePairRow;
export type ThemeNavTarget =
  | "general"
  | "calendar"
  | "signals"
  | "json";
export type SourceGroupId =
  | "app-canvas"
  | "calendar-surface"
  | "calendar-details"
  | "event-panel"
  | "ink"
  | "primary-action"
  | "destructive"
  | "confirm"
  | "warning";

export type SourceGroup = {
  id: SourceGroupId;
  sourceKey: keyof ThemeSources | null;
  title: string;
  description: string;
  navTarget?: Exclude<ThemeNavTarget, "json">;
  rows: GroupRow[];
};

export const APP_TOKEN_INFO: Record<string, TokenInfo> = {
  "--background": {
    title: "App canvas",
    description: "Most views paint their own surface over it",
  },
  "--cal-header-bg": {
    title: "Calendar header",
    description: "Calendar toolbar and day/time headers",
  },
  "--foreground": {
    title: "Text color",
    description: "Applied to text across the app",
  },
  "--card": {
    title: "Card",
    description: "Background of grouped panels, dialogs, and tinted cards",
  },
  "--primary": {
    title: "Primary action",
    description: "Main accent color for highlighted buttons and links",
  },
  "--primary-foreground": {
    title: "Button text",
    description: "Text on primary buttons",
  },
  "--destructive": {
    title: "Destructive",
    description: "Color used for delete actions and warnings",
  },
  "--destructive-foreground": {
    title: "Destructive text",
    description: "Text color on destructive buttons and the title bar close hover",
  },
  "--ring": {
    title: "Focus ring",
    description: "Outline shown around focused inputs and buttons",
  },
  "--event-panel-bg": {
    title: "Event panel surface",
    description: "Background of the event creation/edit panel",
  },
  "--event-panel-contrast": {
    title: "Event panel section header",
    description: "Background strip behind section rows",
  },
  "--event-panel-text": {
    title: "Event panel body text",
    description: "Overrides --foreground inside the panel",
  },
  "--event-panel-muted-text": {
    title: "Event panel muted text",
    description: "Secondary text color inside the panel",
  },
  "--action-confirm": {
    title: "Confirm action",
    description:
      "Background of the Save button and the active scope selector pill in the event panel",
  },
  "--action-confirm-foreground": {
    title: "Confirm action text",
    description: "Text color on the Save button and active scope pill",
  },
  "--action-danger-armed": {
    title: "Armed delete",
    description:
      "Background of the delete button once it has been armed (click-again-to-confirm state)",
  },
  "--action-danger-armed-foreground": {
    title: "Armed delete text",
    description: "Text color on the delete button in its armed state",
  },
  "--status-accepted": {
    title: "Accepted attendee",
    description: "Status tile color for accepted attendees on a calendar event",
  },
  "--status-accepted-foreground": {
    title: "Accepted attendee text",
    description: "Text color on the accepted attendance tile",
  },
  "--status-tentative": {
    title: "Tentative attendee",
    description: "Status tile color for tentative attendees on a calendar event",
  },
  "--status-tentative-foreground": {
    title: "Tentative attendee text",
    description: "Text color on the tentative attendance tile",
  },
  "--status-declined": {
    title: "Declined attendee",
    description: "Status tile color for declined attendees on a calendar event",
  },
  "--status-declined-foreground": {
    title: "Declined attendee text",
    description: "Text color on the declined attendance tile",
  },
};

export const CALENDAR_TOKEN_INFO: Record<string, TokenInfo> = {
  "--cal-bg": {
    title: "Calendar background",
    description: "Background of the calendar grid",
  },
  "--cal-gridline": {
    title: "Grid lines",
    description: "Color of the hour and day separator lines",
  },
  "--cal-time-label": {
    title: "Time labels",
    description: "Hour numbers down the side of the calendar",
  },
  "--cal-current-time": {
    title: "Now line",
    description: "Horizontal line marking the current time",
  },
  "--cal-timeline-rail": {
    title: "Empty rail",
    description: "Color of empty parts of the pomodoro session rail",
  },
  "--cal-timeline-break": {
    title: "Break marker",
    description: "Color of break segments on the session rail",
  },
  "--cal-timeline-focus": {
    title: "Focus marker",
    description: "Color of focus segments on the session rail",
  },
};

export const SOURCE_GROUPS: SourceGroup[] = [
  {
    id: "app-canvas",
    sourceKey: "canvas",
    title: "App canvas",
    description:
      "Dominant background color, most surfaces tint automatically from it",
    navTarget: "general",
    rows: [
      { kind: "single", key: "--background", scope: "app" },
      { kind: "single", key: "--cal-header-bg", scope: "app" },
      { kind: "single", key: "--card", scope: "app" },
      {
        kind: "pair",
        bg: "--popover",
        fg: "--popover-foreground",
        title: "Popover",
        description: "Dropdowns, menus, and floating panels",
        scope: "app",
      },
      {
        kind: "pair",
        bg: "--secondary",
        fg: "--secondary-foreground",
        title: "Secondary surface",
        description: "Less emphasized buttons",
        scope: "app",
      },
      {
        kind: "pair",
        bg: "--muted",
        fg: "--muted-foreground",
        title: "Muted surface",
        description: "Subtle wells and the default hint-text color",
        scope: "app",
        target: 3,
      },
      {
        kind: "pair",
        bg: "--accent",
        fg: "--accent-foreground",
        title: "Hover highlight",
        description: "Soft tint shown when hovering rows and buttons",
        scope: "app",
      },
      { kind: "single", key: "--ring", scope: "app" },
      {
        kind: "pair",
        bg: "--sidebar",
        fg: "--sidebar-foreground",
        title: "Title bar",
        description: "Top frame of the app window",
        scope: "app",
      },
      {
        kind: "pair",
        bg: "--sidebar-accent",
        fg: "--sidebar-accent-foreground",
        title: "Title bar hover",
        description: "Tint shown when hovering title bar buttons",
        scope: "app",
      },
    ],
  },
  {
    id: "calendar-surface",
    sourceKey: null,
    title: "Calendar surface",
    description: "Calendar background, gridlines, and time labels",
    rows: [
      { kind: "single", key: "--cal-bg", scope: "cal" },
      { kind: "single", key: "--cal-gridline", scope: "cal" },
      { kind: "single", key: "--cal-time-label", scope: "cal" },
    ],
  },
  {
    id: "calendar-details",
    sourceKey: null,
    title: "Calendar details",
    description: "Rail states and accents on the calendar grid",
    rows: [
      { kind: "single", key: "--cal-current-time", scope: "cal" },
      { kind: "single", key: "--cal-timeline-rail", scope: "cal" },
      { kind: "single", key: "--cal-timeline-break", scope: "cal" },
      { kind: "single", key: "--cal-timeline-focus", scope: "cal" },
    ],
  },
  {
    id: "event-panel",
    sourceKey: null,
    title: "Event panel",
    description:
      "Surfaces on the event creation and edit panel opened from the calendar",
    rows: [
      { kind: "single", key: "--event-panel-bg", scope: "app" },
      { kind: "single", key: "--event-panel-contrast", scope: "app" },
      { kind: "single", key: "--event-panel-text", scope: "app" },
      { kind: "single", key: "--event-panel-muted-text", scope: "app" },
    ],
  },
  {
    id: "ink",
    sourceKey: "ink",
    title: "Ink",
    description: "Base text color",
    navTarget: "signals",
    rows: [{ kind: "single", key: "--foreground", scope: "app" }],
  },
  {
    id: "primary-action",
    sourceKey: "primary",
    title: "Primary action",
    description: "Accent for highlighted buttons and links",
    rows: [{ kind: "single", key: "--primary-foreground", scope: "app" }],
  },
  {
    id: "destructive",
    sourceKey: null,
    title: "Destructive",
    description: "Danger signal",
    rows: [
      {
        kind: "source-pair",
        bg: "--destructive",
        fg: "--destructive-foreground",
        bgSource: "destructive",
        fgSource: "destructiveText",
        title: "Destructive",
        description: "Delete actions, armed delete, and declined status",
        scope: "app",
        target: 3,
      },
    ],
  },
  {
    id: "confirm",
    sourceKey: null,
    title: "Confirm",
    description: "Positive signal",
    rows: [
      {
        kind: "source-pair",
        bg: "--action-confirm",
        fg: "--action-confirm-foreground",
        bgSource: "confirm",
        fgSource: "confirmText",
        title: "Confirm",
        description: "Save actions, active pills, and accepted status",
        scope: "app",
        target: 3,
      },
    ],
  },
  {
    id: "warning",
    sourceKey: null,
    title: "Warning",
    description: "Caution signal",
    rows: [
      {
        kind: "source-pair",
        bg: "--status-tentative",
        fg: "--status-tentative-foreground",
        bgSource: "warning",
        fgSource: "warningText",
        title: "Warning",
        description: "Tentative status and caution surfaces",
        scope: "app",
        target: 3,
      },
    ],
  },
];

const TEXT_ACTION_GROUP_IDS = new Set<SourceGroupId>([
  "ink",
  "primary-action",
  "destructive",
  "confirm",
  "warning",
]);

const CALENDAR_GROUP_IDS = new Set<SourceGroupId>([
  "calendar-surface",
  "calendar-details",
  "event-panel",
]);

export function isTextActionGroup(group: SourceGroup): boolean {
  return TEXT_ACTION_GROUP_IDS.has(group.id);
}

export function isCalendarGroup(group: SourceGroup): boolean {
  return CALENDAR_GROUP_IDS.has(group.id);
}

export function humanizeToken(token: string): string {
  const stripped = token.replace(/^--/, "").replace(/^cal-/, "");
  const spaced = stripped.replace(/-/g, " ");
  if (spaced.length === 0) return spaced;
  return spaced.charAt(0).toUpperCase() + spaced.slice(1);
}

export function tokenInfo(row: GroupSingleRow): TokenInfo {
  const lookup = row.scope === "app" ? APP_TOKEN_INFO : CALENDAR_TOKEN_INFO;
  return lookup[row.key] ?? { title: humanizeToken(row.key), description: "" };
}

export function localizedThemeNavItems(
  t: Translate,
): ReadonlyArray<{ label: string; target: ThemeNavTarget }> {
  return [
    { label: t("settings.theme.editor.nav.general"), target: "general" },
    { label: t("settings.theme.editor.nav.calendar"), target: "calendar" },
    { label: t("settings.theme.editor.nav.signals"), target: "signals" },
    { label: t("settings.theme.editor.nav.json"), target: "json" },
  ];
}

export function localizedThemeSectionLabel(
  target: ThemeNavTarget,
  t: Translate,
): string {
  switch (target) {
    case "general":
      return t("settings.theme.editor.nav.general");
    case "calendar":
      return t("settings.theme.editor.nav.calendar");
    case "signals":
      return t("settings.theme.editor.nav.signals");
    case "json":
      return t("settings.theme.editor.nav.json");
  }
}

export function localizedCalendarDefaultOptions(
  t: Translate,
): ReadonlyArray<{ mode: CalendarColorDefaultMode; label: string }> {
  return [
    { mode: "light", label: t("settings.theme.editor.calendarDefault.light") },
    { mode: "dark", label: t("settings.theme.editor.calendarDefault.dark") },
    {
      mode: "app-canvas",
      label: t("settings.theme.editor.calendarDefault.appCanvas"),
    },
    {
      mode: "custom",
      label: t("settings.theme.editor.calendarDefault.custom"),
    },
  ];
}

export function localizedTokenInfo(
  row: GroupSingleRow,
  t: Translate,
): TokenInfo {
  switch (`${row.scope}:${row.key}`) {
    case "app:--background":
      return {
        title: t("settings.theme.editor.token.appCanvas.title"),
        description: t("settings.theme.editor.token.appCanvas.description"),
      };
    case "app:--cal-header-bg":
      return {
        title: t("settings.theme.editor.token.calendarHeader.title"),
        description: t("settings.theme.editor.token.calendarHeader.description"),
      };
    case "app:--foreground":
      return {
        title: t("settings.theme.editor.token.textColor.title"),
        description: t("settings.theme.editor.token.textColor.description"),
      };
    case "app:--card":
      return {
        title: t("settings.theme.editor.token.card.title"),
        description: t("settings.theme.editor.token.card.description"),
      };
    case "app:--primary":
      return {
        title: t("settings.theme.editor.token.primaryAction.title"),
        description: t("settings.theme.editor.token.primaryAction.description"),
      };
    case "app:--primary-foreground":
      return {
        title: t("settings.theme.editor.token.buttonText.title"),
        description: t("settings.theme.editor.token.buttonText.description"),
      };
    case "app:--destructive":
      return {
        title: t("settings.theme.editor.token.destructive.title"),
        description: t("settings.theme.editor.token.destructive.description"),
      };
    case "app:--destructive-foreground":
      return {
        title: t("settings.theme.editor.token.destructiveText.title"),
        description: t("settings.theme.editor.token.destructiveText.description"),
      };
    case "app:--ring":
      return {
        title: t("settings.theme.editor.token.focusRing.title"),
        description: t("settings.theme.editor.token.focusRing.description"),
      };
    case "app:--event-panel-bg":
      return {
        title: t("settings.theme.editor.token.eventPanelSurface.title"),
        description: t(
          "settings.theme.editor.token.eventPanelSurface.description",
        ),
      };
    case "app:--event-panel-contrast":
      return {
        title: t("settings.theme.editor.token.eventPanelSectionHeader.title"),
        description: t(
          "settings.theme.editor.token.eventPanelSectionHeader.description",
        ),
      };
    case "app:--event-panel-text":
      return {
        title: t("settings.theme.editor.token.eventPanelBodyText.title"),
        description: t(
          "settings.theme.editor.token.eventPanelBodyText.description",
        ),
      };
    case "app:--event-panel-muted-text":
      return {
        title: t("settings.theme.editor.token.eventPanelMutedText.title"),
        description: t(
          "settings.theme.editor.token.eventPanelMutedText.description",
        ),
      };
    case "app:--action-confirm":
      return {
        title: t("settings.theme.editor.token.confirmAction.title"),
        description: t("settings.theme.editor.token.confirmAction.description"),
      };
    case "app:--action-confirm-foreground":
      return {
        title: t("settings.theme.editor.token.confirmActionText.title"),
        description: t(
          "settings.theme.editor.token.confirmActionText.description",
        ),
      };
    case "app:--action-danger-armed":
      return {
        title: t("settings.theme.editor.token.armedDelete.title"),
        description: t("settings.theme.editor.token.armedDelete.description"),
      };
    case "app:--action-danger-armed-foreground":
      return {
        title: t("settings.theme.editor.token.armedDeleteText.title"),
        description: t("settings.theme.editor.token.armedDeleteText.description"),
      };
    case "app:--status-accepted":
      return {
        title: t("settings.theme.editor.token.acceptedAttendee.title"),
        description: t(
          "settings.theme.editor.token.acceptedAttendee.description",
        ),
      };
    case "app:--status-accepted-foreground":
      return {
        title: t("settings.theme.editor.token.acceptedAttendeeText.title"),
        description: t(
          "settings.theme.editor.token.acceptedAttendeeText.description",
        ),
      };
    case "app:--status-tentative":
      return {
        title: t("settings.theme.editor.token.tentativeAttendee.title"),
        description: t(
          "settings.theme.editor.token.tentativeAttendee.description",
        ),
      };
    case "app:--status-tentative-foreground":
      return {
        title: t("settings.theme.editor.token.tentativeAttendeeText.title"),
        description: t(
          "settings.theme.editor.token.tentativeAttendeeText.description",
        ),
      };
    case "app:--status-declined":
      return {
        title: t("settings.theme.editor.token.declinedAttendee.title"),
        description: t(
          "settings.theme.editor.token.declinedAttendee.description",
        ),
      };
    case "app:--status-declined-foreground":
      return {
        title: t("settings.theme.editor.token.declinedAttendeeText.title"),
        description: t(
          "settings.theme.editor.token.declinedAttendeeText.description",
        ),
      };
    case "cal:--cal-bg":
      return {
        title: t("settings.theme.editor.token.calendarBackground.title"),
        description: t(
          "settings.theme.editor.token.calendarBackground.description",
        ),
      };
    case "cal:--cal-gridline":
      return {
        title: t("settings.theme.editor.token.gridLines.title"),
        description: t("settings.theme.editor.token.gridLines.description"),
      };
    case "cal:--cal-time-label":
      return {
        title: t("settings.theme.editor.token.timeLabels.title"),
        description: t("settings.theme.editor.token.timeLabels.description"),
      };
    case "cal:--cal-current-time":
      return {
        title: t("settings.theme.editor.token.nowLine.title"),
        description: t("settings.theme.editor.token.nowLine.description"),
      };
    case "cal:--cal-timeline-rail":
      return {
        title: t("settings.theme.editor.token.emptyRail.title"),
        description: t("settings.theme.editor.token.emptyRail.description"),
      };
    case "cal:--cal-timeline-break":
      return {
        title: t("settings.theme.editor.token.breakMarker.title"),
        description: t("settings.theme.editor.token.breakMarker.description"),
      };
    case "cal:--cal-timeline-focus":
      return {
        title: t("settings.theme.editor.token.focusMarker.title"),
        description: t("settings.theme.editor.token.focusMarker.description"),
      };
    default:
      return tokenInfo(row);
  }
}

function localizedSourceGroupInfo(
  group: SourceGroup,
  t: Translate,
): TokenInfo {
  switch (group.id) {
    case "app-canvas":
      return {
        title: t("settings.theme.editor.group.appCanvas.title"),
        description: t("settings.theme.editor.group.appCanvas.description"),
      };
    case "calendar-surface":
      return {
        title: t("settings.theme.editor.group.calendarSurface.title"),
        description: t("settings.theme.editor.group.calendarSurface.description"),
      };
    case "calendar-details":
      return {
        title: t("settings.theme.editor.group.calendarDetails.title"),
        description: t("settings.theme.editor.group.calendarDetails.description"),
      };
    case "event-panel":
      return {
        title: t("settings.theme.editor.group.eventPanel.title"),
        description: t("settings.theme.editor.group.eventPanel.description"),
      };
    case "ink":
      return {
        title: t("settings.theme.editor.group.ink.title"),
        description: t("settings.theme.editor.group.ink.description"),
      };
    case "primary-action":
      return {
        title: t("settings.theme.editor.group.primaryAction.title"),
        description: t("settings.theme.editor.group.primaryAction.description"),
      };
    case "destructive":
      return {
        title: t("settings.theme.editor.group.destructive.title"),
        description: t("settings.theme.editor.group.destructive.description"),
      };
    case "confirm":
      return {
        title: t("settings.theme.editor.group.confirm.title"),
        description: t("settings.theme.editor.group.confirm.description"),
      };
    case "warning":
      return {
        title: t("settings.theme.editor.group.warning.title"),
        description: t("settings.theme.editor.group.warning.description"),
      };
  }
}

function localizedPairInfo(
  row: GroupPairRow | GroupSourcePairRow,
  t: Translate,
): TokenInfo {
  switch (row.kind === "source-pair" ? row.bgSource : row.bg) {
    case "--popover":
      return {
        title: t("settings.theme.editor.pair.popover.title"),
        description: t("settings.theme.editor.pair.popover.description"),
      };
    case "--secondary":
      return {
        title: t("settings.theme.editor.pair.secondarySurface.title"),
        description: t("settings.theme.editor.pair.secondarySurface.description"),
      };
    case "--muted":
      return {
        title: t("settings.theme.editor.pair.mutedSurface.title"),
        description: t("settings.theme.editor.pair.mutedSurface.description"),
      };
    case "--accent":
      return {
        title: t("settings.theme.editor.pair.hoverHighlight.title"),
        description: t("settings.theme.editor.pair.hoverHighlight.description"),
      };
    case "--sidebar":
      return {
        title: t("settings.theme.editor.pair.titleBar.title"),
        description: t("settings.theme.editor.pair.titleBar.description"),
      };
    case "--sidebar-accent":
      return {
        title: t("settings.theme.editor.pair.titleBarHover.title"),
        description: t("settings.theme.editor.pair.titleBarHover.description"),
      };
    case "destructive":
      return {
        title: t("settings.theme.editor.pair.destructive.title"),
        description: t("settings.theme.editor.pair.destructive.description"),
      };
    case "confirm":
      return {
        title: t("settings.theme.editor.pair.confirm.title"),
        description: t("settings.theme.editor.pair.confirm.description"),
      };
    case "warning":
      return {
        title: t("settings.theme.editor.pair.warning.title"),
        description: t("settings.theme.editor.pair.warning.description"),
      };
    default:
      return { title: row.title, description: row.description };
  }
}

function localizedGroupRow(row: GroupRow, t: Translate): GroupRow {
  if (row.kind === "single") return row;
  return { ...row, ...localizedPairInfo(row, t) };
}

export function localizedSourceGroups(t: Translate): SourceGroup[] {
  return SOURCE_GROUPS.map((group) => ({
    ...group,
    ...localizedSourceGroupInfo(group, t),
    rows: group.rows.map((row) => localizedGroupRow(row, t)),
  }));
}
