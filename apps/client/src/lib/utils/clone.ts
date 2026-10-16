/**
 * Deep copy plain data: primitives, arrays, and plain objects, with other objects such as dates copied by
 * `structuredClone`. It reads through Svelte state proxies, which Chromium's `structuredClone` rejects (the Android
 * WebView), so use it instead of `structuredClone` for any value that may come from component or store state.
 */
export function cloneData<T>(value: T): T {
  return cloneValue(value) as T;
}

function cloneValue(value: unknown): unknown {
  if (value === null || typeof value !== "object") return value;
  if (Array.isArray(value)) return value.map(cloneValue);
  const prototype: unknown = Object.getPrototypeOf(value);
  if (prototype === Object.prototype || prototype === null) {
    const copy: Record<string, unknown> = prototype === null ? Object.create(null) : {};
    for (const [key, entry] of Object.entries(value)) copy[key] = cloneValue(entry);
    return copy;
  }
  return structuredClone(value);
}
