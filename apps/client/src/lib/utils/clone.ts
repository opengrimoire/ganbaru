/**
 * Deep copy plain data: primitives, arrays, and plain objects, with other objects such as dates copied by
 * `structuredClone`. It reads through Svelte state proxies, which Chromium's `structuredClone` rejects (the Android
 * WebView), so use it instead of `structuredClone` for any value that may come from component or store state. Own keys,
 * `__proto__` included, are copied as data properties and never change the prototype of the copy.
 */
export function cloneData<T>(value: T): T {
  return cloneValue(value) as T;
}

function cloneValue(value: unknown): unknown {
  if (value === null || typeof value !== "object") return value;
  if (Array.isArray(value)) return value.map(cloneValue);
  const prototype: unknown = Object.getPrototypeOf(value);
  if (prototype === Object.prototype || prototype === null) {
    const entries = Object.entries(value).map(([key, entry]): [string, unknown] => [key, cloneValue(entry)]);
    return prototype === null ? Object.assign(Object.create(null), Object.fromEntries(entries)) : Object.fromEntries(entries);
  }
  return structuredClone(value);
}
