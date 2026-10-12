import {
  createLazyComponentLoader,
  type LazyComponentImporter,
} from "$lib/lazy-component-loader";
import type {
  LoadedProjectOptionalComponent,
  ProjectOptionalComponentKind,
} from "./component-registry-contracts";

export type {
  LoadedProjectOptionalComponent,
  ProjectOptionalComponentKind,
} from "./component-registry-contracts";

type LoadedMobileOptionalComponent = Extract<
  LoadedProjectOptionalComponent,
  { kind: "toolbar" | "bulk-actions" }
>;

const MOBILE_OPTIONAL_IMPORTERS = {
  toolbar: () => import("./ProjectToolbarPanels.svelte")
    .then((module) => ({ default: { kind: "toolbar" as const, component: module.default } })),
  "bulk-actions": () => import("./ProjectBulkActionController.svelte")
    .then((module) => ({
      default: { kind: "bulk-actions" as const, component: module.default },
    })),
} satisfies Readonly<Record<
  "toolbar" | "bulk-actions",
  LazyComponentImporter<LoadedMobileOptionalComponent>
>>;

const mobileOptionalLoader = createLazyComponentLoader<
  "toolbar" | "bulk-actions",
  LoadedMobileOptionalComponent
>(MOBILE_OPTIONAL_IMPORTERS);

function unsupportedMobileComponent(
  kind: Exclude<ProjectOptionalComponentKind, "toolbar" | "bulk-actions">,
): Promise<LoadedProjectOptionalComponent> {
  return Promise.reject(new Error(`Project ${kind} is unavailable in the mobile composition`));
}

/** Load a shared optional Project surface supported by the mobile composition. */
export function loadProjectOptionalComponent(
  kind: ProjectOptionalComponentKind,
): Promise<LoadedProjectOptionalComponent> {
  if (kind === "toolbar" || kind === "bulk-actions") return mobileOptionalLoader.load(kind);
  return unsupportedMobileComponent(kind);
}

/** Retry a shared optional Project surface supported by the mobile composition. */
export function retryProjectOptionalComponent(
  kind: ProjectOptionalComponentKind,
): Promise<LoadedProjectOptionalComponent> {
  if (kind === "toolbar" || kind === "bulk-actions") return mobileOptionalLoader.retry(kind);
  return unsupportedMobileComponent(kind);
}

/** Report whether a supported mobile Project constructor is cached. */
export function projectOptionalComponentHasLoaded(kind: ProjectOptionalComponentKind): boolean {
  return (kind === "toolbar" || kind === "bulk-actions") && mobileOptionalLoader.hasLoaded(kind);
}
