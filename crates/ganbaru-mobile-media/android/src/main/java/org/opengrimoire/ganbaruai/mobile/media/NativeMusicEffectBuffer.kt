package org.opengrimoire.ganbaruai.mobile.media

/** Bounded service admission with one scheduled main-thread drain and explicit start failure. */
internal class NativeMusicEffectBuffer<Owner : Any, Effect : Any>(
  private val capacity: Int,
  private val post: (() -> Unit) -> Boolean,
  private val consume: (Owner, Effect) -> Unit,
  private val nowMs: () -> Long = { System.nanoTime() / 1_000_000 },
  private val startTimeoutMs: Long = 10_000,
) {
  private val gate = Any()
  private val pending = ArrayDeque<Effect>()
  private var owner: Owner? = null
  private var starting = false
  private var scheduled = false
  private var epoch = 0L
  private var startupDeadline: Long? = null
  private var inFlight: Effect? = null

  init { require(capacity > 0 && startTimeoutMs > 0) }

  fun isActive(): Boolean = synchronized(gate) { expireStartLocked(); owner != null || starting }

  fun beginStart(): Boolean = synchronized(gate) {
    expireStartLocked()
    if (owner != null || starting) return@synchronized false
    starting = true
    startupDeadline = nowMs() + startTimeoutMs
    true
  }

  fun failedStart() = synchronized(gate) {
    if (owner == null) clearLocked()
  }

  fun created(value: Owner) = synchronized(gate) {
    if (owner != null && owner !== value) clearLocked()
    owner = value
    starting = false
    startupDeadline = null
    scheduleLocked()
  }

  fun destroyed(value: Owner) = synchronized(gate) {
    if (owner === value) { owner = null; clearLocked() }
  }

  fun offer(effect: Effect): Boolean = synchronized(gate) {
    expireStartLocked()
    if ((owner == null && !starting) || pending.size >= capacity) return@synchronized false
    pending.addLast(effect)
    if (!scheduleLocked()) {
      pending.removeLast()
      return@synchronized false
    }
    true
  }

  /** True proves matching work is absent from both queued and executing SDK work. */
  fun cancel(matches: (Effect) -> Boolean): Boolean = synchronized(gate) {
    if (inFlight?.let(matches) == true) return@synchronized false
    pending.removeAll(matches)
    true
  }

  private fun clearLocked() {
    starting = false
    startupDeadline = null
    scheduled = false
    pending.clear()
    epoch++
  }

  private fun expireStartLocked() {
    if (owner == null && startupDeadline?.let { nowMs() >= it } == true) clearLocked()
  }

  private fun scheduleLocked(): Boolean {
    if (owner == null || scheduled || pending.isEmpty()) return true
    scheduled = true
    val captured = epoch
    if (!post { drain(captured) }) {
      scheduled = false
      return false
    }
    return true
  }

  /** Service callbacks and consumption share the Android main thread. */
  private fun drain(captured: Long) {
    repeat(capacity) {
      val delivery = synchronized(gate) {
        if (captured != epoch) return
        val current = owner
        if (current == null || pending.isEmpty()) { scheduled = false; return }
        val effect = pending.removeFirst()
        inFlight = effect
        current to effect
      }
      try { consume(delivery.first, delivery.second) }
      finally { synchronized(gate) { inFlight = null } }
    }
    synchronized(gate) {
      if (captured == epoch) { scheduled = false; scheduleLocked() }
    }
  }
}
