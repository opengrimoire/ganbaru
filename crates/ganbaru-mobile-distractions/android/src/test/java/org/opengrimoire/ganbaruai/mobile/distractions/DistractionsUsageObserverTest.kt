package org.opengrimoire.ganbaruai.mobile.distractions

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class DistractionsUsageObserverTest {
  @Test
  fun countsPassiveVisibleTimeWithoutInteractionEvents() {
    val result = reconcile(
      events = listOf(event(UsageObservationEventKind.ACTIVITY_RESUMED, 1_000L)),
      start = 1_000L,
      end = 61_000L,
    )

    assertEquals(listOf(interval(APP, 1_000L, 61_000L)), result.intervals)
    assertEquals(setOf(APP), result.visiblePackages)
  }

  @Test
  fun keepsCountingAPausedActivityUntilItBecomesInvisible() {
    val result = reconcile(
      events = listOf(
        event(UsageObservationEventKind.ACTIVITY_RESUMED, 1_000L),
        event(UsageObservationEventKind.ACTIVITY_PAUSED, 10_000L),
        event(
          UsageObservationEventKind.ACTIVITY_RESUMED,
          11_000L,
          packageName = "com.android.systemui",
        ),
      ),
      start = 1_000L,
      end = 61_000L,
    )

    assertEquals(listOf(interval(APP, 1_000L, 61_000L)), result.intervals)
  }

  @Test
  fun stopsCountingWhenTheActivityBecomesInvisible() {
    val result = reconcile(
      events = listOf(
        event(UsageObservationEventKind.ACTIVITY_RESUMED, 1_000L),
        event(UsageObservationEventKind.ACTIVITY_STOPPED, 21_000L),
      ),
      start = 1_000L,
      end = 61_000L,
    )

    assertEquals(listOf(interval(APP, 1_000L, 21_000L)), result.intervals)
    assertTrue(result.visiblePackages.isEmpty())
  }

  @Test
  fun excludesScreenOffAndLockedIntervals() {
    val result = reconcile(
      events = listOf(
        event(UsageObservationEventKind.ACTIVITY_RESUMED, 0L),
        event(UsageObservationEventKind.KEYGUARD_SHOWN, 10_000L, packageName = null),
        event(UsageObservationEventKind.SCREEN_NON_INTERACTIVE, 11_000L, packageName = null),
        event(UsageObservationEventKind.SCREEN_INTERACTIVE, 20_000L, packageName = null),
        event(UsageObservationEventKind.KEYGUARD_HIDDEN, 25_000L, packageName = null),
      ),
      start = 0L,
      end = 30_000L,
    )

    assertEquals(
      listOf(
        interval(APP, 0L, 10_000L),
        interval(APP, 25_000L, 30_000L),
      ),
      result.intervals,
    )
  }

  @Test
  fun countsEverySelectedPackageVisibleInMultiWindow() {
    val secondPackage = "com.example.video"
    val result = DistractionsUsageObserver.reconcile(
      initialState = UsageObservationState(),
      events = listOf(
        event(UsageObservationEventKind.ACTIVITY_RESUMED, 0L),
        event(
          UsageObservationEventKind.ACTIVITY_RESUMED,
          10_000L,
          packageName = secondPackage,
        ),
        event(UsageObservationEventKind.ACTIVITY_PAUSED, 11_000L),
      ),
      observedPackages = setOf(APP, secondPackage),
      usagePackages = setOf(APP, secondPackage),
      intervalStartEpochMs = 0L,
      intervalEndEpochMs = 20_000L,
    )

    assertEquals(
      listOf(
        interval(APP, 0L, 20_000L),
        interval(secondPackage, 10_000L, 20_000L),
      ),
      result.intervals,
    )
  }

  @Test
  fun clipsRecoveredActivityTimeToTheDurableCursor() {
    val result = reconcile(
      events = listOf(
        event(UsageObservationEventKind.ACTIVITY_RESUMED, 100L),
        event(UsageObservationEventKind.ACTIVITY_STOPPED, 3_000L),
      ),
      start = 1_000L,
      end = 5_000L,
    )

    assertEquals(listOf(interval(APP, 1_000L, 3_000L)), result.intervals)
  }

  @Test
  fun dailyRolloverSeedsAnActivityThatRemainedVisible() {
    val result = reconcile(
      events = listOf(
        event(
          UsageObservationEventKind.ACTIVITY_VISIBLE_ROLLOVER,
          500L,
          activityName = null,
        ),
        event(UsageObservationEventKind.ACTIVITY_STOPPED, 3_000L),
      ),
      start = 1_000L,
      end = 5_000L,
    )

    assertEquals(listOf(interval(APP, 1_000L, 3_000L)), result.intervals)
    assertTrue(result.visiblePackages.isEmpty())
  }

  @Test
  fun replaysTheDeliveryOverlapWithoutCountingTheCursorTwice() {
    val first = reconcile(
      events = listOf(event(UsageObservationEventKind.ACTIVITY_RESUMED, 0L)),
      start = 0L,
      end = 10_000L,
    )
    val second = DistractionsUsageObserver.reconcile(
      initialState = first.state,
      events = listOf(event(UsageObservationEventKind.ACTIVITY_RESUMED, 0L)),
      observedPackages = setOf(APP),
      usagePackages = setOf(APP),
      intervalStartEpochMs = 10_000L,
      intervalEndEpochMs = 20_000L,
    )

    assertEquals(listOf(interval(APP, 10_000L, 20_000L)), second.intervals)
  }

  @Test
  fun deviceShutdownClearsUnclosedActivities() {
    val result = reconcile(
      events = listOf(
        event(UsageObservationEventKind.ACTIVITY_RESUMED, 0L),
        event(UsageObservationEventKind.DEVICE_SHUTDOWN, 5_000L, packageName = null),
      ),
      start = 0L,
      end = 20_000L,
    )

    assertEquals(listOf(interval(APP, 0L, 5_000L)), result.intervals)
    assertTrue(result.visiblePackages.isEmpty())
  }

  private fun reconcile(
    events: List<UsageObservationEvent>,
    start: Long,
    end: Long,
  ): UsageObservationResult = DistractionsUsageObserver.reconcile(
    initialState = UsageObservationState(),
    events = events,
    observedPackages = setOf(APP),
    usagePackages = setOf(APP),
    intervalStartEpochMs = start,
    intervalEndEpochMs = end,
  )

  private fun event(
    kind: UsageObservationEventKind,
    timestamp: Long,
    packageName: String? = APP,
    activityName: String? = packageName?.let { "$it.MainActivity" },
  ) = UsageObservationEvent(
    packageName = packageName,
    activityName = activityName,
    kind = kind,
    timestampEpochMs = timestamp,
  )

  private fun interval(packageName: String, startedAt: Long, endedAt: Long) =
    VisibleUsageInterval(packageName, startedAt, endedAt)

  private companion object {
    const val APP = "com.example.reader"
  }
}
