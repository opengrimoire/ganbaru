package org.opengrimoire.ganbaru.mobile.sync

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SyncSchedulingTest {
  private fun result(outcome: PassOutcome, vararg names: String) = PassResult(outcome, names.toList())

  @Test
  fun unknownOrMissingOutcomesAreFailures() {
    assertEquals(PassOutcome.SYNCED, PassOutcome.fromWire("synced"))
    assertEquals(PassOutcome.DELEGATED, PassOutcome.fromWire("delegated"))
    assertEquals(PassOutcome.FAILED, PassOutcome.fromWire("Synced"))
    assertEquals(PassOutcome.FAILED, PassOutcome.fromWire(null))
  }

  @Test
  fun passesRunOnlyForLinkedDevicesOnLocalNetworks() {
    assertTrue(SyncScheduling.shouldRun(linked = true, localNetwork = true))
    assertFalse(SyncScheduling.shouldRun(linked = true, localNetwork = false))
    assertFalse(SyncScheduling.shouldRun(linked = false, localNetwork = true))
  }

  @Test
  fun anUnreachableCoordinatorIsBrowsedOnceWhenItHasNames() {
    val unreachable = result(PassOutcome.UNREACHABLE, "tag")
    assertEquals(PassFollowUp.BROWSE, SyncScheduling.followUp(unreachable, browsed = false))
    assertEquals(PassFollowUp.RETRY, SyncScheduling.followUp(unreachable, browsed = true))
    assertEquals(PassFollowUp.RETRY, SyncScheduling.followUp(result(PassOutcome.UNREACHABLE), browsed = false))
  }

  @Test
  fun completedPassesFinishAndFailuresRetry() {
    for (outcome in listOf(PassOutcome.SYNCED, PassOutcome.IDLE, PassOutcome.DELEGATED)) {
      assertEquals(PassFollowUp.FINISH, SyncScheduling.followUp(result(outcome), browsed = false))
    }
    assertEquals(PassFollowUp.RETRY, SyncScheduling.followUp(PassResult.FAILED, browsed = false))
  }

  @Test
  fun onlyOneTimeRunsRetryAndOnlyBeforeTheirLastAttempt() {
    assertEquals(RunEnd.RETRY, SyncScheduling.runEnd(PassFollowUp.RETRY, oneTime = true, runAttemptCount = 0))
    assertEquals(
      RunEnd.SUCCESS,
      SyncScheduling.runEnd(PassFollowUp.RETRY, oneTime = true, runAttemptCount = SyncScheduling.MAX_ONE_TIME_ATTEMPTS - 1),
    )
    assertEquals(RunEnd.SUCCESS, SyncScheduling.runEnd(PassFollowUp.RETRY, oneTime = false, runAttemptCount = 0))
    assertEquals(RunEnd.SUCCESS, SyncScheduling.runEnd(PassFollowUp.FINISH, oneTime = true, runAttemptCount = 0))
  }

  @Test
  fun expeditedWorkIsRequestedOnlyWhereItRunsAsAJob() {
    assertEquals(OneTimeRequest.ORDINARY, SyncScheduling.oneTimeRequest(29))
    assertEquals(OneTimeRequest.ORDINARY, SyncScheduling.oneTimeRequest(30))
    assertEquals(OneTimeRequest.EXPEDITED, SyncScheduling.oneTimeRequest(31))
    assertEquals(OneTimeRequest.EXPEDITED, SyncScheduling.oneTimeRequest(36))
  }

  @Test
  fun registeringANetworkCallbackIsNotAChange() {
    val observations = NetworkObservations()
    assertFalse(observations.observe(1, setOf("192.168.1.2/24")))
    assertFalse(observations.observe(1, setOf("192.168.1.2/24")))
    assertTrue(observations.observe(1, setOf("192.168.1.3/24")))
    assertTrue(observations.observe(2, setOf("192.168.1.3/24")))
    observations.lost()
    assertTrue(observations.observe(2, setOf("192.168.1.3/24")))
  }

  @Test
  fun aLossBeforeTheFirstObservationIsNotAChange() {
    val observations = NetworkObservations()
    observations.lost()
    assertFalse(observations.observe(1, emptySet()))
  }
}
