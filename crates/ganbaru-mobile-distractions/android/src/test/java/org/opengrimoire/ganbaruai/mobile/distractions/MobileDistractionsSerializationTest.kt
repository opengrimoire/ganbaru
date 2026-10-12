package org.opengrimoire.ganbaruai.mobile.distractions

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class MobileDistractionsSerializationTest {
  @Test
  fun createsPlainLaunchableAppResponse() {
    val result = launchableAppResponse("YouTube", "com.google.android.youtube")

    assertEquals(setOf("name", "packageName"), result.keys)
    assertEquals("YouTube", result["name"])
    assertEquals("com.google.android.youtube", result["packageName"])
  }

  @Test
  fun createsPlainPendingEventResponse() {
    val result = JournalEvent(
      id = "event-1",
      kind = "block",
      packageName = "com.google.android.youtube",
      displayName = "YouTube",
      startedAtEpochMs = 1_000L,
      elapsedSeconds = 0,
      localDate = "2026-08-29",
      occurredAtEpochMs = 1_000L,
      reason = "schedule",
      ruleId = null,
      runId = "run-1",
      phase = "focus",
      vaultId = "vault-1",
    ).toResponse()

    assertEquals(
      setOf(
        "id",
        "kind",
        "packageName",
        "displayName",
        "startedAtMs",
        "elapsedSeconds",
        "localDate",
        "occurredAtMs",
        "reason",
        "ruleId",
        "runId",
        "phase",
        "vaultId",
      ),
      result.keys,
    )
    assertEquals("event-1", result["id"])
    assertEquals("com.google.android.youtube", result["packageName"])
    assertNull(result["ruleId"])
  }
}
