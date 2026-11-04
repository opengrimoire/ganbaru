package org.opengrimoire.ganbaru.mobile.media

/**
 * One keep-alive binding to the playback service while the native Music session runs.
 *
 * The service stays exported for system media controllers, so any app can send `stopService` to
 * it. Android does not destroy a started service while an auto-create binding remains, so holding
 * this binding keeps another app from ending playback that way. The binding intent has no action,
 * for which Media3 returns no binder, so it exposes no session interface.
 *
 * @param bind Binds the service and reports whether the system accepted the binding.
 * @param unbind Releases the connection passed to [bind].
 */
internal class NativeMusicServiceBinding(
  private val bind: () -> Boolean,
  private val unbind: () -> Unit,
) {
  private var held = false

  /** Whether the binding is currently held. */
  val isHeld: Boolean
    @Synchronized get() = held

  /**
   * Holds the binding unless it is already held.
   *
   * @return Whether the binding is held afterwards. A refused binding leaves playback running
   * without the protection.
   */
  @Synchronized
  fun hold(): Boolean {
    if (held) return true
    held = try { bind() } catch (error: SecurityException) { false }
    if (!held) releaseRefused()
    return held
  }

  /** Releases a held binding so the service can stop. Does nothing when none is held. */
  @Synchronized
  fun release() {
    if (!held) return
    held = false
    unbind()
  }

  /**
   * Android asks callers to unbind even after a refused binding, while older releases reject an
   * unbind for a connection they never registered.
   */
  private fun releaseRefused() {
    try { unbind() } catch (error: IllegalArgumentException) { }
  }
}
