package org.opengrimoire.ganbaruai.mobile.distractions

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class DistractionsEvaluatorTest {
  @Test
  fun acceptedCombinedUsageAddsOnlyNewOfflineLocalUsage() {
    assertEquals(75L, combinedUsageSinceAcceptance(60, 20, 35))
    assertEquals(60L, combinedUsageSinceAcceptance(60, 20, 10))
  }

  @Test
  fun delayedPublicationKeepsUsageRecordedAfterSnapshotCapture() {
    val accepted = AcceptedUsage("2026-10-02", "2026-10-02", 100, 20)
    val localWhenPublished = 35L
    assertEquals(115L, combinedUsageSinceAcceptance(
      accepted.usedSeconds, checkNotNull(accepted.localUsedSecondsAtCapture), localWhenPublished,
    ))
  }

  @Test
  fun combinedCountersPreserveValuesAboveTheSignedIntRange() {
    assertEquals(3_000_000_015L, combinedUsageSinceAcceptance(3_000_000_000, 20, 35))
  }

  @Test(expected = ArithmeticException::class)
  fun combinedCountersRejectOverflowInsteadOfWrappingToAnAvailableBudget() {
    combinedUsageSinceAcceptance(Long.MAX_VALUE, 0, 1)
  }

  @Test(expected = IllegalArgumentException::class)
  fun combinedCountersRejectNegativeEvidence() {
    combinedUsageSinceAcceptance(100, -1, 20)
  }

  private val schedule = MobileSchedule(
    enabled = true,
    blockDuringFocus = true,
    blockDuringShortBreaks = false,
    blockDuringLongBreaks = true,
    pauseDuringFocusPause = true,
    blockedApps = listOf(
      MobileAppRule("YouTube", "com.google.android.youtube", enabled = true),
    ),
  )

  @Test
  fun blocksSelectedAppDuringEnabledActivePhase() {
    assertTrue(DistractionsEvaluator.isBlockedBySchedule(
      schedule,
      phase(phase = "focus"),
      "com.google.android.youtube",
      NOW,
    ))
  }

  @Test
  fun followsPerPhaseSchedule() {
    assertFalse(DistractionsEvaluator.isBlockedBySchedule(
      schedule,
      phase(phase = "short_break"),
      "com.google.android.youtube",
      NOW,
    ))
    assertTrue(DistractionsEvaluator.isBlockedBySchedule(
      schedule,
      phase(phase = "long_break"),
      "com.google.android.youtube",
      NOW,
    ))
  }

  @Test
  fun failsOpenForPausedOrExpiredProjection() {
    assertFalse(DistractionsEvaluator.isBlockedBySchedule(
      schedule,
      phase(phase = "focus", running = false),
      "com.google.android.youtube",
      NOW,
    ))
    assertFalse(DistractionsEvaluator.isBlockedBySchedule(
      schedule,
      phase(phase = "focus", validUntilEpochMs = NOW),
      "com.google.android.youtube",
      NOW,
    ))
  }

  @Test
  fun ignoresAppsWithoutAnEnabledExactPackageRule() {
    assertFalse(DistractionsEvaluator.isBlockedBySchedule(
      schedule,
      phase(phase = "focus"),
      "com.example.video",
      NOW,
    ))
    assertFalse(DistractionsEvaluator.isBlockedBySchedule(
      schedule.copy(blockedApps = schedule.blockedApps.map { it.copy(enabled = false) }),
      phase(phase = "focus"),
      "com.google.android.youtube",
      NOW,
    ))
  }

  @Test
  fun recoveryObservesScheduledAppsWithoutCountingThemAsLimitUsage() {
    val rules = rules(
      limitsEnabled = true,
      limits = listOf(MobileLimit(
        id = "video",
        name = "Video",
        enabled = true,
        minutesPerDay = 60,
        minutesPerWeek = null,
        packages = setOf("com.example.video"),
      )),
    )

    val scope = DistractionsRecovery.scope(rules)

    assertEquals(setOf("com.example.video"), scope.usagePackages)
    assertEquals(
      setOf("com.example.video", "com.google.android.youtube"),
      scope.observedPackages,
    )
  }

  @Test
  fun recoveryExcludesDisabledRulesAndUsesThreeDayEventWindow() {
    val rules = rules(
      limitsEnabled = false,
      schedule = schedule.copy(
        blockedApps = schedule.blockedApps + MobileAppRule(
          "Disabled",
          "com.example.disabled",
          enabled = false,
        ),
      ),
    )

    val scope = DistractionsRecovery.scope(rules)

    assertTrue(scope.usagePackages.isEmpty())
    assertEquals(setOf("com.google.android.youtube"), scope.observedPackages)
    assertEquals(NOW - 3 * 24 * 60 * 60 * 1_000L, DistractionsRecovery.queryStart(NOW))
    assertEquals(0L, DistractionsRecovery.queryStart(1_000L))
    assertEquals(NOW - 5_000L, DistractionsRecovery.incrementalQueryStart(NOW))
    assertEquals(0L, DistractionsRecovery.incrementalQueryStart(1_000L))
  }

  private fun phase(
    phase: String,
    running: Boolean = true,
    validUntilEpochMs: Long = NOW + 60_000L,
  ) = PomodoroPhaseState(
    active = true,
    runId = "run-1",
    phase = phase,
    running = running,
    validUntilEpochMs = validUntilEpochMs,
  )

  private fun rules(
    limitsEnabled: Boolean,
    limits: List<MobileLimit> = emptyList(),
    schedule: MobileSchedule = this.schedule,
  ) = DistractionsRulesSnapshot(
    vaultId = "vault-1",
    revision = "revision-1",
    generatedAtEpochMs = NOW,
    mobile = schedule,
    limitsEnabled = limitsEnabled,
    limits = limits,
    copy = DistractionsCopy(
      channelName = "Distractions",
      channelDescription = "Distractions",
      blockedMessage = "Blocked",
      limitMessage = "Limit reached",
    ),
  )

  private companion object {
    const val NOW = 1_788_041_200_000L
  }
}
