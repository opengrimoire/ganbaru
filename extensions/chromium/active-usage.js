/**
 * The website interval the extension is measuring: the focused tab's host since `startedAtMs`.
 *
 * @typedef {{ tabId: number | null, host: string, startedAtMs: number }} ActiveUsage
 */

/**
 * The page usage would be counted against now.
 *
 * @typedef {{ tabId: number | null, host: string }} UsageTarget
 */

/**
 * Storage and browser access the tracker needs, injected so the tracking rules run without the extension APIs.
 *
 * @typedef {object} ActiveUsageTrackerDependencies
 * @property {() => Promise<ActiveUsage | null>} readActiveUsage Reads the stored interval.
 * @property {(usage: ActiveUsage) => Promise<void>} writeActiveUsage Stores a new interval.
 * @property {() => Promise<void>} clearActiveUsage Removes the stored interval.
 * @property {() => Promise<UsageTarget | null>} queryTarget Reads the trackable focused page, or null.
 * @property {(usage: ActiveUsage, endedAtMs: number) => Promise<void>} recordInterval Reports a finished interval.
 * @property {() => number} now Current time in milliseconds.
 */

/**
 * Tracks the focused website interval. Refreshes run one at a time, because several browser listeners fire for one
 * user action and two overlapping refreshes would both report the same interval. A refresh also reports an interval
 * only while the stored one still starts at the time it read, and stores the next interval before reporting the
 * finished one, so a service worker restarted mid-refresh cannot report it a second time.
 *
 * @param {ActiveUsageTrackerDependencies} deps Storage and browser access.
 * @returns {{ refresh: (options?: { forceFlush?: boolean }) => Promise<void> }} The serialized refresh, whose promise
 *   settles after this refresh and logs instead of rejecting.
 */
export function createActiveUsageTracker(deps) {
  /** @type {Promise<void>} */
  let queue = Promise.resolve();

  /**
   * @param {{ forceFlush?: boolean }} options
   * @returns {Promise<void>}
   */
  async function refreshOnce({ forceFlush = false }) {
    const usage = await deps.readActiveUsage();
    const target = await deps.queryTarget();
    const now = deps.now();

    if (!usage) {
      if (target) await deps.writeActiveUsage({ ...target, startedAtMs: now });
      return;
    }

    const sameHost = target?.host === usage.host;
    if (!forceFlush && sameHost) return;

    const stored = await deps.readActiveUsage();
    if (stored?.startedAtMs !== usage.startedAtMs || stored.host !== usage.host) return;

    if (target) {
      await deps.writeActiveUsage({ ...target, startedAtMs: now });
    } else {
      await deps.clearActiveUsage();
    }
    await deps.recordInterval(usage, now);
  }

  return {
    refresh(options = {}) {
      queue = queue.then(() => refreshOnce(options)).catch((error) => {
        console.warn("[ganbaru] active usage refresh failed", error);
      });
      return queue;
    },
  };
}
