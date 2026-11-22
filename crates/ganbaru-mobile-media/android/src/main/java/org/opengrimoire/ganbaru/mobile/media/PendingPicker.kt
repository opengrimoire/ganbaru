package org.opengrimoire.ganbaru.mobile.media

import java.util.concurrent.atomic.AtomicReference

private data class PendingPick<T>(
  val invokeId: Long,
  val request: T,
)

/**
 * Holds the one picker request in flight for a command and the invoke that owns it.
 *
 * A second request is refused while one is active, and only the owning invoke can take or release
 * the request, so a refused or stale call never clears the active pick.
 */
internal class PendingPicker<T>(private val busyMessage: String) {
  private val slot = AtomicReference<PendingPick<T>?>(null)

  /** Records [request] for [invokeId], or throws when another pick is already active. */
  fun begin(invokeId: Long, request: T) {
    check(slot.compareAndSet(null, PendingPick(invokeId, request))) { busyMessage }
  }

  /** Returns and clears the request owned by [invokeId], or null when it owns none. */
  fun take(invokeId: Long): T? {
    val held = slot.get() ?: return null
    if (held.invokeId != invokeId) return null
    return if (slot.compareAndSet(held, null)) held.request else null
  }

  /** Clears the request owned by [invokeId], for example when its picker fails to open. */
  fun release(invokeId: Long) {
    take(invokeId)
  }
}
