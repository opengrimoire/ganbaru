import {
  BUILD_PLATFORM_PROFILE,
  platformHasCapability,
  type PlatformCapability,
  type PlatformProfile,
} from "$lib/platform";

export const APP_VIEWS = ["calendar", "projects", "notes", "chat"] as const;
export type View = (typeof APP_VIEWS)[number];

export const DETACHABLE_TAB_VIEWS = APP_VIEWS;
export type DetachableTabView = (typeof DETACHABLE_TAB_VIEWS)[number];

const VIEW_SET = new Set<string>(APP_VIEWS);
const DETACHABLE_TAB_VIEW_SET = new Set<string>(DETACHABLE_TAB_VIEWS);
const VIEW_CAPABILITIES = {
  calendar: "view.calendar",
  projects: "view.projects",
  notes: "view.notes",
  chat: "view.chat",
} as const satisfies Readonly<Record<View, PlatformCapability>>;

export function isView(value: unknown): value is View {
  return typeof value === "string" && VIEW_SET.has(value);
}

export function isDetachableTabView(value: unknown): value is DetachableTabView {
  return typeof value === "string" && DETACHABLE_TAB_VIEW_SET.has(value);
}

export function viewLabel(view: View): string {
  if (view === "calendar") return "Calendar";
  if (view === "projects") return "Projects";
  if (view === "notes") return "Notes";
  return "Chat";
}

/** Return whether a registered app view is available on a platform profile. */
export function isViewAvailable(
  view: View,
  profile: PlatformProfile = BUILD_PLATFORM_PROFILE,
): boolean {
  return platformHasCapability(profile, VIEW_CAPABILITIES[view]);
}

/** Return app views exposed by a platform profile in stable navigation order. */
export function availableAppViews(
  profile: PlatformProfile = BUILD_PLATFORM_PROFILE,
): View[] {
  return APP_VIEWS.filter((view) => isViewAvailable(view, profile));
}

/** Parse an initial view only when the build profile exposes that destination. */
export function parseInitialViewSearch(
  search: string,
  profile: PlatformProfile = BUILD_PLATFORM_PROFILE,
): View | undefined {
  const params = new URLSearchParams(search);
  const view = params.get("view");
  return isView(view) && isViewAvailable(view, profile) ? view : undefined;
}

export function mainTabViews(
  detachedViews: ReadonlySet<DetachableTabView>,
  profile: PlatformProfile = BUILD_PLATFORM_PROFILE,
): DetachableTabView[] {
  return DETACHABLE_TAB_VIEWS.filter(
    (view) => isViewAvailable(view, profile) && !detachedViews.has(view),
  );
}
