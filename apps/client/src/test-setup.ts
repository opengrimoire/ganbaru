// Lock tests to UTC so date/time assertions are deterministic regardless of
// the developer's system zone. Must be set before any Date/Intl calls.
process.env.TZ = "UTC";

import { Temporal } from "@js-temporal/polyfill";

(globalThis as unknown as { Temporal: typeof Temporal }).Temporal = Temporal;

// jsdom has no layout, so it omits range geometry; an empty box matches what it reports for elements.
const layoutGlobals = globalThis as { Range?: { prototype: object }; DOMRect?: new () => object };
const { Range: RangeClass, DOMRect: DOMRectClass } = layoutGlobals;
if (RangeClass && DOMRectClass && !("getBoundingClientRect" in RangeClass.prototype)) {
  Object.defineProperty(RangeClass.prototype, "getBoundingClientRect", {
    configurable: true,
    value: () => new DOMRectClass(),
  });
}
