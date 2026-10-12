package org.opengrimoire.ganbaruai.mobile.notifications

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class NativeFocusDeliveryScopeTest {
  @Test
  fun queuedRevocationCannotClearANewerProcessAndCurrentRevocationNeedsNoPhase() {
    var process = 100L
    var revocations = 0
    val old = NativeFocusProcessScope(process)
    val source = { nonce: Long -> nonce == process }
    old.publish(source) { revocations++ }
    process = 101
    assertThrows(IllegalStateException::class.java) { old.publish(source) { revocations++ } }
    NativeFocusProcessScope(process).publish(source) { revocations++ }
    assertEquals(2, revocations)
    assertThrows(IllegalArgumentException::class.java) { NativeFocusProcessScope(0) }
    assertThrows(IllegalStateException::class.java) {
      NativeFocusProcessScope(process).publish({ false }) { revocations++ }
    }
    assertEquals(2, revocations)
  }
  @Test
  fun queuedPublicationChecksTheSourceAfterAnEarlierRevisionReplacesIt() {
    var revision = 10L
    var publications = 0
    val source = { process: Long, generation: Long, candidate: Long -> process == 100L && generation == 3L && candidate == revision }
    val queued = { NativeFocusDeliveryScope(100, 3, 10).publish(source) { publications++ } }
    revision = 11
    assertThrows(IllegalStateException::class.java) { queued() }
    assertEquals(0, publications)
    NativeFocusDeliveryScope(100, 3, 11).publish(source) { publications++ }
    assertEquals(1, publications)
  }

  @Test
  fun revocationOrSourceFailureCannotPublishAndDoesNotBecomeRetainedPermission() {
    var writable = true
    var publications = 0
    val scope = NativeFocusDeliveryScope(100, 1, 0)
    val source = { _: Long, _: Long, _: Long -> writable }
    scope.publish(source) { publications++ }
    writable = false
    assertThrows(IllegalStateException::class.java) { scope.publish(source) { publications++ } }
    assertThrows(IllegalArgumentException::class.java) {
      scope.publish({ _, _, _ -> errorSource() }) { publications++ }
    }
    assertEquals(1, publications)
  }

  @Test
  fun invalidScopesCannotReachPublication() {
    listOf(0L to 0L, -1L to 0L, 1L to -1L).forEach { (generation, revision) ->
      assertThrows(IllegalArgumentException::class.java) { NativeFocusDeliveryScope(100, generation, revision) }
    }
    assertThrows(IllegalArgumentException::class.java) { NativeFocusDeliveryScope(0, 1, 0) }
  }

  @Test
  fun restartedProcessRejectsAnOldEnvelopeEvenWhenPublicationCountersCoincide() {
    var process = 100L
    var publications = 0
    val scope = NativeFocusDeliveryScope(process, 3, 10)
    val source = { nonce: Long, generation: Long, revision: Long -> nonce == process && generation == 3L && revision == 10L }
    process = 101
    assertThrows(IllegalStateException::class.java) { scope.publish(source) { publications++ } }
    NativeFocusDeliveryScope(process, 3, 10).publish(source) { publications++ }
    assertEquals(1, publications)
  }

  private fun errorSource(): Boolean = throw IllegalArgumentException("Authority unavailable")
}
