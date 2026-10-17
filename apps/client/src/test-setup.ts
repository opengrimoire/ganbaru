// Lock tests to UTC so date/time assertions are deterministic regardless of
// the developer's system zone. Must be set before any Date/Intl calls.
process.env.TZ = "UTC";

import { Temporal } from "@js-temporal/polyfill";

(globalThis as unknown as { Temporal: typeof Temporal }).Temporal = Temporal;
