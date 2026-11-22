package org.opengrimoire.ganbaru.mobile.media

import java.util.concurrent.atomic.AtomicReference

/** The request a tree scan answered: the folder and the limits it was scanned with. */
internal data class TreeScanRequest(
  val treeUri: String,
  val maxFiles: Int,
  val maxDepth: Int,
)

private data class PickedTreeScan<T>(
  val request: TreeScanRequest,
  val tree: T,
  val scannedAtMs: Long,
)

/**
 * Holds the scan a folder pick produced so the refresh that follows the pick can reuse it.
 *
 * The scan is reused once, only by a request with the same folder and limits, and only within
 * [reuseWindowMs], so a narrow pick or an old pick never answers a later full refresh.
 */
internal class PickedTreeScanCache<T>(
  private val reuseWindowMs: Long,
  private val nowMs: () -> Long,
) {
  private val slot = AtomicReference<PickedTreeScan<T>?>(null)

  /** Replaces any held scan with the scan that answered [request]. */
  fun store(request: TreeScanRequest, tree: T) {
    slot.set(PickedTreeScan(request, tree, nowMs()))
  }

  /** Drops any held scan, for example when a new pick starts. */
  fun clear() {
    slot.set(null)
  }

  /**
   * Returns and drops the held scan when it answered exactly [request] and is still fresh.
   *
   * An expired scan is dropped; a scan for another request is kept for its own refresh.
   */
  fun take(request: TreeScanRequest): T? {
    while (true) {
      val held = slot.get() ?: return null
      val expired = nowMs() - held.scannedAtMs !in 0..reuseWindowMs
      if (!expired && held.request != request) return null
      if (slot.compareAndSet(held, null)) return if (expired) null else held.tree
    }
  }
}
