import {
  beginLazyComponentLoad,
  rejectLazyComponentLoad,
  resolveLazyComponentLoad,
  type LazyComponentLoadState,
} from "$lib/lazy-component-loader";
import type { getProjects } from "$lib/stores/projects.svelte";
import {
  loadProjectOptionalComponent,
  retryProjectOptionalComponent,
  type LoadedProjectOptionalComponent,
  type ProjectOptionalComponentKind,
} from "$lib/components/projects/component-registry";

type ProjectsStore = ReturnType<typeof getProjects>;
type OptionalLoadState = LazyComponentLoadState<
  ProjectOptionalComponentKind,
  LoadedProjectOptionalComponent
>;

export interface ProjectRouteLoaders {
  loadOptional: typeof loadProjectOptionalComponent;
  retryOptional: typeof retryProjectOptionalComponent;
}

const DEFAULT_LOADERS: ProjectRouteLoaders = {
  loadOptional: loadProjectOptionalComponent,
  retryOptional: retryProjectOptionalComponent,
};

/** Coordinates independent code, toolbar-data, and task-detail loading identities. */
export class ProjectRouteLoadController {
  readonly #projects: ProjectsStore;
  readonly #loaders: ProjectRouteLoaders;
  optionalStates = $state<Partial<Record<ProjectOptionalComponentKind, OptionalLoadState>>>({});
  toolbarDataError = $state<string | null>(null);
  taskDetailDataError = $state<string | null>(null);
  #toolbarDataRequest = 0;
  #taskDetailDataRequest = 0;
  #taskDetailIdentity: string | null = null;

  constructor(projects: ProjectsStore, loaders: ProjectRouteLoaders = DEFAULT_LOADERS) {
    this.#projects = projects;
    this.#loaders = loaders;
  }

  optionalState(kind: ProjectOptionalComponentKind): OptionalLoadState | null {
    return this.optionalStates[kind] ?? null;
  }

  requestOptional(kind: ProjectOptionalComponentKind, retry = false): void {
    const current = this.optionalStates[kind] ?? null;
    if (!retry && current) return;
    const loading = beginLazyComponentLoad(current, kind);
    this.optionalStates = { ...this.optionalStates, [kind]: loading };
    const request = retry ? this.#loaders.retryOptional(kind) : this.#loaders.loadOptional(kind);
    void request.then((component) => {
      const active = this.optionalStates[kind];
      if (!active) return;
      const next = resolveLazyComponentLoad(active, kind, loading.requestId, component);
      if (next !== active) this.optionalStates = { ...this.optionalStates, [kind]: next };
    }).catch((error: unknown) => {
      const active = this.optionalStates[kind];
      if (!active) return;
      const next = rejectLazyComponentLoad(active, kind, loading.requestId, error);
      if (next !== active) this.optionalStates = { ...this.optionalStates, [kind]: next };
      console.error(`Failed to load optional Project surface ${kind}:`, error);
    });
  }

  async requestToolbarData(
    projectId: string | null,
    isCurrent: () => boolean = () => true,
  ): Promise<void> {
    if (!projectId) return;
    const identity = ++this.#toolbarDataRequest;
    this.toolbarDataError = null;
    try {
      await this.#projects.ensureProjectToolbarData(projectId);
    } catch (error) {
      if (identity !== this.#toolbarDataRequest
        || this.#projects.selectedProject?.id !== projectId
        || !isCurrent()) return;
      this.toolbarDataError = error instanceof Error ? error.message : String(error);
      console.error("load optional Project toolbar data failed", error);
    }
  }

  async requestTaskDetailData(projectId: string | null, taskId: string | null): Promise<void> {
    if (!projectId || !taskId) return;
    const identity = ++this.#taskDetailDataRequest;
    this.#taskDetailIdentity = `${projectId}:${taskId}`;
    this.taskDetailDataError = null;
    try {
      await this.#projects.ensureTaskDetailData(projectId, taskId);
    } catch (error) {
      if (identity !== this.#taskDetailDataRequest
        || this.#taskDetailIdentity !== `${projectId}:${taskId}`
        || this.#projects.selectedProject?.id !== projectId) return;
      this.taskDetailDataError = error instanceof Error ? error.message : String(error);
      console.error("load optional Project task detail data failed", error);
    }
  }

  invalidateToolbarData(): void {
    this.#toolbarDataRequest += 1;
    this.toolbarDataError = null;
  }

  invalidateTaskDetailData(): void {
    this.#taskDetailDataRequest += 1;
    this.#taskDetailIdentity = null;
    this.taskDetailDataError = null;
  }
}
