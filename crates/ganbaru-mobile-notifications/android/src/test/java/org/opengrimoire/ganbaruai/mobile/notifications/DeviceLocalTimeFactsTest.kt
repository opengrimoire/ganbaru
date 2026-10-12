package org.opengrimoire.ganbaruai.mobile.notifications

import java.time.Instant
import java.time.ZoneId
import org.junit.Assert.assertEquals
import org.junit.Test

class DeviceLocalTimeFactsTest {
  @Test
  fun historicalDatesUseTheirOwnDstOffsetsAndStableEnglishSeedDate() {
    val instants = listOf(
      "2026-03-08T06:59:00Z",
      "2026-03-08T07:01:00Z",
      "2026-11-01T05:30:00Z",
      "2026-11-01T06:30:00Z",
    ).map { Instant.parse(it).toEpochMilli() }
    val facts = DeviceLocalTimeFacts.resolve(instants, ZoneId.of("America/New_York"))
    assertEquals(listOf(1, 3, 1, 1), facts.map { it.hour })
    assertEquals("Sun Mar 08 2026", facts[0].dateString)
    assertEquals("Sun Nov 01 2026", facts[3].dateString)
  }

  @Test
  fun localMidnightBelongsToTheDeviceDate() {
    val instants = listOf("2026-10-02T05:59:59Z", "2026-10-02T06:00:00Z")
      .map { Instant.parse(it).toEpochMilli() }
    val facts = DeviceLocalTimeFacts.resolve(instants, ZoneId.of("America/Monterrey"))
    assertEquals(listOf("2026-10-01", "2026-10-02"), facts.map { it.dateKey })
    assertEquals(listOf(23, 0), facts.map { it.hour })
  }

  @Test(expected = IllegalArgumentException::class)
  fun excessHistoryIsRejectedBeforeExpandingLocalFacts() {
    DeviceLocalTimeFacts.resolve(List(DeviceLocalTimeFacts.MAX_INSTANTS + 1) { 0L })
  }
}
