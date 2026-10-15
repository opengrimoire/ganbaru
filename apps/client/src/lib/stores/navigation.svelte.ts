import {
  availableAppViews,
  isViewAvailable,
  parseInitialViewSearch,
  type View,
} from "$lib/navigation";

export type { View } from "$lib/navigation";

/**
 * Decides where a navigation request lands. Returning false rejects the request, for example when the
 * kind lives in another window that the router focused instead.
 */
export type NavigationRouter = (view: View) => boolean;

export interface NavigationRouterRegistration {
  /** Show a kind without routing, for the router's own tab activation. */
  commit(view: View): boolean;
  unregister(): void;
}

function initialView(): View {
  const fallback = availableAppViews()[0] ?? "calendar";
  if (typeof window === "undefined") return fallback;
  return parseInitialViewSearch(window.location.search) ?? fallback;
}

let currentView = $state<View>(initialView());
let router: NavigationRouter | null = null;

function setCurrentView(view: View): boolean {
  if (!isViewAvailable(view)) return false;
  currentView = view;
  return true;
}

function routeView(view: View): boolean {
  if (!isViewAvailable(view)) return false;
  return router ? router(view) : setCurrentView(view);
}

/** Let the workspace tabs decide which tab every navigation request lands in. */
export function registerNavigationRouter(next: NavigationRouter): NavigationRouterRegistration {
  router = next;
  return {
    commit: setCurrentView,
    unregister() {
      if (router === next) router = null;
    },
  };
}

export function getNavigation() {
  return {
    get current() {
      return currentView;
    },
    set current(view: View) {
      routeView(view);
    },
    navigate(view: View) {
      return routeView(view);
    },
  };
}
