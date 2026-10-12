package org.opengrimoire.ganbaruai.mobile.media

import org.junit.Assert.*
import org.junit.Test

class NativeMusicEffectBufferTest {
  private class Fixture(capacity: Int = 3) {
    val callbacks = ArrayDeque<() -> Unit>()
    val delivered = mutableListOf<Pair<Any, Int>>()
    var acceptsPosts = true
    val effects = NativeMusicEffectBuffer<Any, Int>(capacity,
      { action -> if (acceptsPosts) { callbacks.addLast(action); true } else false },
      { owner, effect -> delivered.add(owner to effect) },
    )
    fun drain() { while (callbacks.isNotEmpty()) callbacks.removeFirst()() }
  }

  @Test
  fun startupAdmissionIsBoundedBeforeDispatchReturnsAndDrainsInOrder() {
    val fixture = Fixture()
    assertFalse(fixture.effects.offer(0))
    assertTrue(fixture.effects.beginStart())
    assertFalse(fixture.effects.beginStart())
    assertTrue(fixture.effects.offer(1))
    assertTrue(fixture.effects.offer(2))
    assertTrue(fixture.effects.offer(3))
    assertFalse(fixture.effects.offer(4))
    assertTrue(fixture.callbacks.isEmpty())
    val owner = Any()
    fixture.effects.created(owner)
    assertEquals(1, fixture.callbacks.size)
    fixture.drain()
    assertEquals(listOf(owner to 1, owner to 2, owner to 3), fixture.delivered)
    assertTrue(fixture.effects.offer(4))
    fixture.drain()
    assertEquals(owner to 4, fixture.delivered.last())
  }

  @Test
  fun activeServiceCoalescesPostsAndKeepsOverflowOutOfTheQueue() {
    val fixture = Fixture()
    val owner = Any()
    fixture.effects.created(owner)
    assertTrue(fixture.effects.offer(1))
    assertTrue(fixture.effects.offer(2))
    assertTrue(fixture.effects.offer(3))
    assertFalse(fixture.effects.offer(4))
    assertEquals(1, fixture.callbacks.size)
    fixture.drain()
    assertEquals(listOf(1, 2, 3), fixture.delivered.map { it.second })
  }

  @Test
  fun destroyedServiceAndOldPostedDrainsCannotReachItsReplacement() {
    val fixture = Fixture()
    val old = Any()
    fixture.effects.created(old)
    fixture.effects.offer(1)
    val obsoleteDrain = fixture.callbacks.removeFirst()
    fixture.effects.destroyed(old)
    assertFalse(fixture.effects.isActive())
    assertFalse(fixture.effects.offer(2))
    val replacement = Any()
    fixture.effects.beginStart()
    fixture.effects.offer(3)
    fixture.effects.created(replacement)
    fixture.effects.destroyed(old)
    obsoleteDrain()
    assertTrue(fixture.delivered.isEmpty())
    assertTrue(fixture.effects.isActive())
    fixture.drain()
    assertEquals(listOf(replacement to 3), fixture.delivered)
  }

  @Test
  fun failedStartDiscardsOnlyThatAttemptAndDoesNotInventAControl() {
    val fixture = Fixture()
    fixture.effects.beginStart()
    fixture.effects.offer(1)
    fixture.effects.failedStart()
    assertFalse(fixture.effects.isActive())
    assertFalse(fixture.effects.offer(2))
    fixture.effects.beginStart()
    fixture.effects.offer(3)
    val owner = Any()
    fixture.effects.created(owner)
    fixture.drain()
    assertEquals(listOf(owner to 3), fixture.delivered)
  }

  @Test
  fun rejectedMainThreadPostReportsFailureWithoutRetainingTheRejectedEffect() {
    val fixture = Fixture()
    fixture.effects.created(Any())
    fixture.acceptsPosts = false
    assertFalse(fixture.effects.offer(1))
    fixture.acceptsPosts = true
    assertTrue(fixture.effects.offer(2))
    fixture.drain()
    assertEquals(listOf(2), fixture.delivered.map { it.second })
  }

  @Test
  fun canceledQueuedWorkCannotReachAReplacementAndCancellationPreservesOtherWork() {
    val fixture = Fixture()
    fixture.effects.created(Any())
    fixture.effects.offer(1)
    fixture.effects.offer(2)
    assertTrue(fixture.effects.cancel { it == 1 })
    fixture.drain()
    assertEquals(listOf(2), fixture.delivered.map { it.second })
    assertTrue(fixture.effects.cancel { it == 1 })
  }

  @Test
  fun cancellationCannotReportDrainedWhileTheSdkConsumerIsExecuting() {
    lateinit var buffer: NativeMusicEffectBuffer<Any, Int>
    var during = true
    var after = false
    val callbacks = ArrayDeque<() -> Unit>()
    buffer = NativeMusicEffectBuffer(2,
      { action -> callbacks.addLast(action); true },
      { _, effect -> during = buffer.cancel { it == effect } },
    )
    buffer.created(Any())
    buffer.offer(1)
    callbacks.removeFirst()()
    after = buffer.cancel { it == 1 }
    assertFalse(during)
    assertTrue(after)
  }

  @Test
  fun stalledStartupExpiresAndCannotReplayItsAcceptedEffectsIntoALaterService() {
    var now = 100L
    val callbacks = ArrayDeque<() -> Unit>()
    val delivered = mutableListOf<Int>()
    val buffer = NativeMusicEffectBuffer<Any, Int>(2,
      { action -> callbacks.addLast(action); true }, { _, effect -> delivered.add(effect) },
      { now }, 10,
    )
    assertTrue(buffer.beginStart())
    assertTrue(buffer.offer(1))
    now = 109
    assertTrue(buffer.isActive())
    now = 110
    assertFalse(buffer.isActive())
    assertFalse(buffer.offer(2))
    buffer.created(Any())
    assertTrue(buffer.isActive())
    assertTrue(buffer.offer(3))
    while (callbacks.isNotEmpty()) callbacks.removeFirst()()
    assertEquals(listOf(3), delivered)
  }
}
