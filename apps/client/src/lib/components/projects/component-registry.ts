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

const OPTIONAL_IMPORTERS = {
  toolbar: () => import("./ProjectToolbarPanels.svelte")
    .then((module) => ({ default: { kind: "toolbar" as const, component: module.default } })),
  "bulk-actions": () => import("./ProjectBulkActionController.svelte")
    .then((module) => ({
      default: { kind: "bulk-actions" as const, component: module.default },
    })),
  "task-finder": () => import("$lib/components/projects/pickers/ProjectTaskFinder.svelte")
    .then((module) => ({
      default: { kind: "task-finder" as const, component: module.default },
    })),
} satisfies Readonly<Record<
  ProjectOptionalComponentKind,
  LazyComponentImporter<LoadedProjectOptionalComponent>
>>;

const optionalLoader = createLazyComponentLoader<
  ProjectOptionalComponentKind,
  LoadedProjectOptionalComponent
>(OPTIONAL_IMPORTERS);

/** Loads and caches one optional Projects surface constructor. */
export function loadProjectOptionalComponent(
  kind: ProjectOptionalComponentKind,
): Promise<LoadedProjectOptionalComponent> {
  return optionalLoader.load(kind);
}

/** Retries a failed optional Projects surface import. */
export function retryProjectOptionalComponent(
  kind: ProjectOptionalComponentKind,
): Promise<LoadedProjectOptionalComponent> {
  return optionalLoader.retry(kind);
}

/** Reports whether an optional Projects constructor is cached. */
export function projectOptionalComponentHasLoaded(
  kind: ProjectOptionalComponentKind,
): boolean {
  return optionalLoader.hasLoaded(kind);
}
