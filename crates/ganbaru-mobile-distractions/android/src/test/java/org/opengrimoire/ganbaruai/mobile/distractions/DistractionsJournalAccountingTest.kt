package org.opengrimoire.ganbaruai.mobile.distractions

import org.junit.Assert.assertEquals
import org.junit.Test

class DistractionsJournalAccountingTest {
  @Test
  fun compactionPreservesEverySecondAcrossFullDayParts() {
    val total = 2L * 86_400L + 17L
    val parts = compactedUsageParts(total)
    assertEquals(listOf(86_400, 86_400, 17), parts)
    assertEquals(total, parts.sumOf(Int::toLong))
    assertEquals(emptyList<Int>(), compactedUsageParts(0))
  }

  @Test
  fun compactionPreservesTheLargestAdmittedJournal() {
    val total = 2_000L * 86_400L
    val parts = compactedUsageParts(total)
    assertEquals(2_000, parts.size)
    assertEquals(total, parts.sumOf(Int::toLong))
  }

  @Test
  fun aTwentyFiveHourLocalDayIsSplitWithoutDroppingTheRepeatedHour() {
    val parts = compactedUsageParts(25L * 60L * 60L)
    assertEquals(listOf(86_400, 3_600), parts)
    assertEquals(90_000L, parts.sumOf(Int::toLong))
  }

  @Test(expected = IllegalArgumentException::class)
  fun compactionRejectsCorruptOrUnboundedEvidence() {
    compactedUsageParts(2_000L * 86_400L + 1L)
  }

  @Test
  fun journalAdmitsTheLastAvailableSlot() {
    requireJournalCapacity(2_000, 2_000)
  }

  @Test(expected = IllegalArgumentException::class)
  fun journalRejectsOverflowBeforeCheckpointCommit() {
    requireJournalCapacity(2_001, 2_000)
  }
}
