package org.opengrimoire.ganbaruai.mobile.notifications

import android.content.Intent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MobileNotificationsPluginTest {
  @Test
  fun exactAlarmAccessIsRequiredFromAndroidTwelve() {
    assertFalse(ExactAlarmCapability.isRequired(30))
    assertTrue(ExactAlarmCapability.isRequired(31))
    assertTrue(ExactAlarmCapability.isRequired(36))
  }

  @Test
  fun backgroundAutostartRegistryCoversMainstreamManufacturerFamilies() {
    val identifiers = listOf(
      "Xiaomi" to "Redmi",
      "Huawei" to "Huawei",
      "Honor" to "HONOR",
      "OPPO" to "OPPO",
      "realme" to "realme",
      "OnePlus" to "OnePlus",
      "vivo" to "iQOO",
      "asus" to "ROG",
      "TECNO" to "TECNO",
      "ZTE" to "nubia",
    )

    identifiers.forEach { (manufacturer, brand) ->
      assertTrue(
        "$manufacturer should have an autostart destination",
        BackgroundExecutionSettingsRegistry.candidates(
          BackgroundSettingsDestination.AUTOSTART,
          manufacturer,
          brand,
        ).isNotEmpty(),
      )
    }
  }

  @Test
  fun samsungUsesItsDocumentedNeverSleepingRouteOnlyForBatteryReview() {
    assertTrue(
      BackgroundExecutionSettingsRegistry.candidates(
        BackgroundSettingsDestination.AUTOSTART,
        " samsung ",
        "SAMSUNG",
      ).isEmpty(),
    )

    val batteryCandidates = BackgroundExecutionSettingsRegistry.candidates(
      BackgroundSettingsDestination.BATTERY,
      "Samsung",
      "Samsung",
    )
    val documentedRoute = batteryCandidates.first() as BackgroundSettingsIntentSpec.Action
    assertEquals(
      "com.samsung.android.sm.ACTION_OPEN_CHECKABLE_LISTACTIVITY",
      documentedRoute.action,
    )
    assertEquals("com.samsung.android.lool", documentedRoute.packageName)
    assertEquals(2, documentedRoute.integerExtras["activity_type"])
  }

  @Test
  fun stockAndroidDoesNotAdvertiseAFalseAutostartControl() {
    assertTrue(
      BackgroundExecutionSettingsRegistry.candidates(
        BackgroundSettingsDestination.AUTOSTART,
        "Google",
        "google",
      ).isEmpty(),
    )
    assertTrue(
      BackgroundExecutionSettingsRegistry.candidates(
        BackgroundSettingsDestination.AUTOSTART,
        "motorola",
        "motorola",
      ).isEmpty(),
    )
  }

  @Test
  fun restoreReceiversAcceptOnlyTheirDeclaredSystemActions() {
    assertTrue(isNotificationRestoreAction(Intent.ACTION_BOOT_COMPLETED))
    assertTrue(isNotificationRestoreAction(Intent.ACTION_MY_PACKAGE_REPLACED))
    assertTrue(isNotificationRestoreAction(Intent.ACTION_TIME_CHANGED))
    assertTrue(isNotificationRestoreAction(Intent.ACTION_TIMEZONE_CHANGED))
    assertFalse(isNotificationRestoreAction(null))
    assertFalse(isNotificationRestoreAction("app.ganbaru.UNTRUSTED_RESTORE"))
  }

  @Test
  fun remindersAreDueOnlyWithinTheirWindowAndDoNotRepeatAfterDelivery() {
    val reminder = PomodoroReminder(
      "reminder-a", "event-a", 1_000, 5_000, "Focus due", "Open the app to start", "Focus", "Reminders",
    )
    reminder.validate()
    assertFalse(reminder.isDue(999, emptySet()))
    assertTrue(reminder.isDue(1_000, emptySet()))
    assertTrue(reminder.isDue(4_999, emptySet()))
    assertFalse(reminder.isDue(5_000, emptySet()))
    assertFalse(reminder.isDue(2_000, setOf(reminder.id)))
  }

  @Test
  fun completionReceiptsPreserveBothIdentitiesWithoutDelimiterCollisions() {
    assertEquals(
      PomodoroNotificationScheduler.completionReceipt("run", "phase"),
      PomodoroNotificationScheduler.completionReceipt("run", "phase"),
    )
    assertFalse(
      PomodoroNotificationScheduler.completionReceipt("a", "b:c") ==
        PomodoroNotificationScheduler.completionReceipt("a:b", "c"),
    )
    assertFalse(
      PomodoroNotificationScheduler.completionReceipt("run", "phase-1") ==
        PomodoroNotificationScheduler.completionReceipt("run", "phase-2"),
    )
  }

  @Test
  fun acceptedPhaseCannotAdvanceFromHistoryOrAStaleOrFutureProjection() {
    val current = projection()
    assertEquals("phase-a", acceptedPomodoroPhase(current, 2_000)?.id)
    assertEquals(null, acceptedPomodoroPhase(current, 500))
    assertEquals(null, acceptedPomodoroPhase(current, 5_000))
    val future = current.phases.single().copy(id = "future", startsAtEpochMs = 5_000, endsAtEpochMs = 8_000)
    assertEquals(null, acceptedPomodoroPhase(current.copy(phases = current.phases + future), 6_000))
    assertEquals(null, acceptedPomodoroPhase(current.copy(phases = listOf(future)), 2_000))
    assertEquals(null, acceptedPomodoroPhase(current.copy(generatedAtEpochMs = 3_000), 2_000))
    assertEquals("phase-a", acceptedPomodoroPhase(current.copy(isRunning = false), 6_000)?.id)
    assertEquals(null, acceptedPomodoroPhase(current.copy(isRunning = false), 10_000))
    assertEquals(null, acceptedPomodoroPhase(current.copy(isRunning = false, remainingSeconds = 0), 6_000))
    assertEquals(null, acceptedPomodoroPhase(current.copy(isRunning = false, generatedAtEpochMs = 7_000), 6_000))
  }

  private fun projection(): PomodoroNotificationProjection = PomodoroNotificationProjection(
    runId = "run-a", eventId = "event-a", eventTitle = "Focus", eventDate = "2026-09-06",
    eventEndsAtEpochMs = 10_000, generatedAtEpochMs = 1_000, isRunning = true,
    remainingSeconds = 4, totalSeconds = 4, configJson = "{}",
    phases = listOf(PomodoroNotificationPhase("phase-a", "focus", 1, 1_000, 5_000)),
    copy = PomodoroNotificationCopy(
      "Focus", "Focus", "Alerts", "Alerts", "Focus", "Short break", "Long break", "Paused",
      "Focus interval ended", "Ready to return", "Open the app to continue",
    ),
  )
}
