export interface LazyComponentModule<Component> {
  default: Component;
}

export type LazyComponentImporter<Component> = () => Promise<LazyComponentModule<Component>>;

export interface LazyComponentLoader<Key extends string, Component> {
  load: (key: Key) => Promise<Component>;
  retry: (key: Key) => Promise<Component>;
  hasLoaded: (key: Key) => boolean;
  read: (key: Key) => Component | null;
}

interface LazyComponentLoadBase<Key extends string> {
  key: Key;
  requestId: number;
}

export interface LoadingLazyComponentState<Key extends string>
  extends LazyComponentLoadBase<Key> {
  status: "loading";
}

export interface ReadyLazyComponentState<Key extends string, Component>
  extends LazyComponentLoadBase<Key> {
  status: "ready";
  component: Component;
}

export interface FailedLazyComponentState<Key extends string>
  extends LazyComponentLoadBase<Key> {
  status: "failed";
  error: unknown;
}

export type LazyComponentLoadState<Key extends string, Component> =
  | LoadingLazyComponentState<Key>
  | ReadyLazyComponentState<Key, Component>
  | FailedLazyComponentState<Key>;

/**
 * Creates a process-local loader that caches successful component modules and
 * deduplicates concurrent imports for the same key.
 */
export function createLazyComponentLoader<Key extends string, Component>(
  importers: Readonly<Record<Key, LazyComponentImporter<Component>>>,
): LazyComponentLoader<Key, Component> {
  const loaded = new Map<Key, Component>();
  const inFlight = new Map<Key, Promise<Component>>();

  function load(key: Key): Promise<Component> {
    if (loaded.has(key)) return Promise.resolve(loaded.get(key) as Component);
    const existing = inFlight.get(key);
    if (existing) return existing;

    let imported: Promise<LazyComponentModule<Component>>;
    try {
      imported = importers[key]();
    } catch (error) {
      imported = Promise.reject(error);
    }
    let request: Promise<Component>;
    request = imported
      .then((module) => {
        loaded.set(key, module.default);
        return module.default;
      })
      .finally(() => {
        if (inFlight.get(key) === request) inFlight.delete(key);
      });
    inFlight.set(key, request);
    return request;
  }

  return {
    load,
    retry: load,
    hasLoaded: (key) => loaded.has(key),
    read: (key) => loaded.get(key) ?? null,
  };
}

/** Starts a new component request and invalidates the previous render state. */
export function beginLazyComponentLoad<Key extends string, Component>(
  current: LazyComponentLoadState<Key, Component> | null,
  key: Key,
): LoadingLazyComponentState<Key> {
  return {
    status: "loading",
    key,
    requestId: (current?.requestId ?? 0) + 1,
  };
}

/** Applies a component only when it belongs to the current request. */
export function resolveLazyComponentLoad<Key extends string, Component>(
  current: LazyComponentLoadState<Key, Component>,
  key: Key,
  requestId: number,
  component: Component,
): LazyComponentLoadState<Key, Component> {
  if (current.key !== key || current.requestId !== requestId) return current;
  return { status: "ready", key, requestId, component };
}

/** Applies an import failure only when it belongs to the current request. */
export function rejectLazyComponentLoad<Key extends string, Component>(
  current: LazyComponentLoadState<Key, Component>,
  key: Key,
  requestId: number,
  error: unknown,
): LazyComponentLoadState<Key, Component> {
  if (current.key !== key || current.requestId !== requestId) return current;
  return { status: "failed", key, requestId, error };
}
