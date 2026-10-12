package org.opengrimoire.ganbaruai.mobile.notifications

import android.util.Log

/** Checks cached committed Rust authority at the actual Android callback boundary. */
internal object NativeFocusAuthority {
  @JvmStatic
  external fun isCurrent(processNonce: Long, generation: Long, revision: Long): Boolean

  @JvmStatic
  external fun isProcessCurrent(processNonce: Long): Boolean

  fun processOrUnavailable(processNonce: Long): Boolean {
    if (processNonce <= 0) return false
    return try {
      isProcessCurrent(processNonce)
    } catch (error: LinkageError) {
      Log.e("GanbaruFocus", "Native Focus process authority is not loaded", error)
      false
    }
  }

  fun currentOrUnavailable(processNonce: Long, generation: Long, revision: Long): Boolean {
    if (processNonce <= 0 || generation <= 0 || revision < 0) return false
    return try {
      isCurrent(processNonce, generation, revision)
    } catch (error: LinkageError) {
      Log.e("GanbaruFocus", "Native Focus authority is not loaded", error)
      false
    }
  }

  fun requireCurrent(processNonce: Long, generation: Long, revision: Long) {
    check(currentOrUnavailable(processNonce, generation, revision)) {
      "Native Focus phase delivery was superseded or expired"
    }
  }
}

/** Revocation remains available when the current owner has cleared phase authority. */
internal data class NativeFocusProcessScope(val processNonce: Long) {
  init { require(processNonce > 0) { "Native Focus process scope is invalid" } }

  fun publish(checkProcess: (Long) -> Boolean, publication: () -> Unit) {
    check(checkProcess(processNonce)) { "Native Focus process was superseded or is unavailable" }
    publication()
  }
}

/** Evaluates the source after queued work is admitted, without caching permission. */
internal data class NativeFocusDeliveryScope(val processNonce: Long, val generation: Long, val revision: Long) {
  init {
    require(processNonce > 0 && generation > 0 && revision >= 0) { "Native Focus delivery scope is invalid" }
  }

  fun publish(checkAuthority: (Long, Long, Long) -> Boolean, publication: () -> Unit) {
    check(checkAuthority(processNonce, generation, revision)) {
      "Native Focus phase delivery was superseded or expired"
    }
    publication()
  }
}
