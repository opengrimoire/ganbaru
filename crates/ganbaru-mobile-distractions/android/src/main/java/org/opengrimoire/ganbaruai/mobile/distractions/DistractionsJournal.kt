package org.opengrimoire.ganbaruai.mobile.distractions

import android.content.ContentValues
import android.content.Context
import android.database.sqlite.SQLiteDatabase
import android.database.sqlite.SQLiteOpenHelper
import java.time.Instant
import java.time.ZoneId
import java.util.UUID

internal data class JournalEvent(
  val id: String,
  val kind: String,
  val packageName: String,
  val displayName: String,
  val startedAtEpochMs: Long,
  val elapsedSeconds: Int,
  val localDate: String,
  val occurredAtEpochMs: Long,
  val reason: String?,
  val ruleId: String?,
  val runId: String?,
  val phase: String?,
  val vaultId: String,
)

internal data class JournalUsageInterval(
  val packageName: String,
  val displayName: String,
  val startedAtEpochMs: Long,
  val endedAtEpochMs: Long,
)

internal class DistractionsJournal(context: Context) : SQLiteOpenHelper(
  context,
  "ganbaru-distractions-runtime.sqlite",
  null,
  2,
) {
  override fun onCreate(db: SQLiteDatabase) {
    db.execSQL(
      """
      CREATE TABLE journal_events (
        id TEXT PRIMARY KEY,
        kind TEXT NOT NULL CHECK(kind IN ('usage', 'block')),
        package_name TEXT NOT NULL,
        display_name TEXT NOT NULL,
        started_at INTEGER NOT NULL,
        elapsed_seconds INTEGER NOT NULL,
        local_date TEXT NOT NULL,
        occurred_at INTEGER NOT NULL,
        reason TEXT,
        rule_id TEXT,
        run_id TEXT,
        phase TEXT,
        vault_id TEXT NOT NULL,
        exported INTEGER NOT NULL DEFAULT 0 CHECK(exported IN (0, 1))
      )
      """.trimIndent(),
    )
    db.execSQL(
      """
      CREATE TABLE daily_totals (
        package_name TEXT NOT NULL,
        local_date TEXT NOT NULL,
        elapsed_seconds INTEGER NOT NULL,
        PRIMARY KEY(package_name, local_date)
      )
      """.trimIndent(),
    )
    db.execSQL("CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
    db.execSQL("CREATE INDEX journal_events_occurred ON journal_events(occurred_at)")
  }

  override fun onUpgrade(db: SQLiteDatabase, oldVersion: Int, newVersion: Int) {
    error("Unsupported development Distractions journal schema $oldVersion; reset application data for schema $newVersion")
  }

  fun recordUsageBatch(
    vaultId: String,
    intervals: List<JournalUsageInterval>,
    observedAtEpochMs: Long,
  ) {
    require(intervals.size <= MAX_PENDING_EVENTS) { "Too many Distractions usage intervals" }
    writableDatabase.beginTransaction()
    try {
      var insertedEvents = 0
      for (interval in intervals) {
        if (interval.endedAtEpochMs <= interval.startedAtEpochMs) continue
        val normalizedPackage = interval.packageName.lowercase()
        for (slice in splitByLocalDate(interval.startedAtEpochMs, interval.endedAtEpochMs)) {
          var partStart = slice.startEpochMs
          for (elapsedSeconds in compactedUsageParts((slice.endEpochMs - slice.startEpochMs) / 1_000L)) {
            insertedEvents += 1
            requireJournalCapacity(insertedEvents, MAX_PENDING_EVENTS)
            val values = ContentValues().apply {
              put("id", "usage-${UUID.randomUUID()}")
              put("kind", "usage")
              put("package_name", normalizedPackage)
              put("display_name", interval.displayName.take(120))
              put("started_at", partStart)
              put("elapsed_seconds", elapsedSeconds)
              put("local_date", slice.localDate)
              put("occurred_at", interval.endedAtEpochMs)
              put("vault_id", vaultId)
            }
            writableDatabase.insertOrThrow("journal_events", null, values)
            incrementDailyTotal(normalizedPackage, slice.localDate, elapsedSeconds)
            partStart += elapsedSeconds * 1_000L
          }
        }
      }
      compactInTransaction()
      requireJournalCapacity(pendingCount(), MAX_PENDING_EVENTS)
      writeMetadata("lastObservedEpochMs", observedAtEpochMs)
      pruneDailyTotals(observedAtEpochMs)
      writableDatabase.setTransactionSuccessful()
    } finally {
      writableDatabase.endTransaction()
    }
  }

  fun recordBlock(
    packageName: String,
    displayName: String,
    occurredAtEpochMs: Long,
    reason: String,
    ruleId: String?,
    runId: String?,
    phase: String?,
    vaultId: String,
  ) {
    writableDatabase.beginTransaction()
    try {
      writableDatabase.insertOrThrow("journal_events", null, ContentValues().apply {
        put("id", "block-${UUID.randomUUID()}")
        put("kind", "block")
        put("package_name", packageName.lowercase())
        put("display_name", displayName.take(120))
        put("started_at", occurredAtEpochMs)
        put("elapsed_seconds", 0)
        put("local_date", localDate(occurredAtEpochMs))
        put("occurred_at", occurredAtEpochMs)
        put("reason", reason.take(80))
        put("rule_id", ruleId?.take(80))
        put("run_id", runId?.take(128))
        put("phase", phase?.take(32))
        put("vault_id", vaultId)
      })
      compactInTransaction()
      requireJournalCapacity(pendingCount(), MAX_PENDING_EVENTS)
      writableDatabase.setTransactionSuccessful()
    } finally {
      writableDatabase.endTransaction()
    }
  }

  fun usedSeconds(packageNames: Set<String>, startDate: String, endDate: String): Long {
    if (packageNames.isEmpty()) return 0L
    val placeholders = packageNames.joinToString(",") { "?" }
    val args = packageNames.map(String::lowercase) + listOf(startDate, endDate)
    readableDatabase.rawQuery(
      """
      SELECT COALESCE(SUM(elapsed_seconds), 0)
      FROM daily_totals
      WHERE package_name IN ($placeholders) AND local_date BETWEEN ? AND ?
      """.trimIndent(),
      args.toTypedArray(),
    ).use { cursor ->
      return if (cursor.moveToFirst()) cursor.getLong(0) else 0L
    }
  }

  fun exportPending(vaultId: String, usageOnly: Boolean, limit: Int = 200): List<JournalEvent> {
    writableDatabase.beginTransaction()
    try {
      val selection = if (usageOnly) "vault_id = ? AND kind = 'usage'" else "vault_id = ?"
      val events = readEvents(selection, arrayOf(vaultId), limit.coerceIn(1, 500))
      for (event in events) {
        writableDatabase.execSQL("UPDATE journal_events SET exported = 1 WHERE id = ?", arrayOf(event.id))
      }
      writableDatabase.setTransactionSuccessful()
      return events
    } finally {
      writableDatabase.endTransaction()
    }
  }

  fun accountingPending(vaultId: String, startDate: String, endDate: String): List<JournalEvent> {
    val events = readEvents("vault_id = ? AND kind = 'usage' AND local_date BETWEEN ? AND ?",
      arrayOf(vaultId, startDate, endDate), MAX_PENDING_EVENTS + 1)
    check(events.size <= MAX_PENDING_EVENTS) { "Distractions accounting journal exceeds its limit" }
    return events
  }

  fun accountingTotals(startDate: String, endDate: String): List<Triple<String, String, Long>> {
    val rows = mutableListOf<Triple<String, String, Long>>()
    readableDatabase.rawQuery(
      "SELECT package_name, local_date, elapsed_seconds FROM daily_totals WHERE local_date BETWEEN ? AND ? ORDER BY local_date, package_name LIMIT ?",
      arrayOf(startDate, endDate, (MAX_ACCOUNTING_SOURCES + 1).toString()),
    ).use { cursor ->
      while (cursor.moveToNext()) rows += Triple(cursor.getString(0), cursor.getString(1), cursor.getLong(2))
    }
    check(rows.size <= MAX_ACCOUNTING_SOURCES) { "Distractions accounting sources exceed their limit" }
    return rows
  }

  private fun readEvents(selection: String, arguments: Array<String>, limit: Int): List<JournalEvent> {
    val events = mutableListOf<JournalEvent>()
    readableDatabase.query(
      "journal_events",
      EVENT_COLUMNS,
      selection,
      arguments,
      null,
      null,
      "CASE WHEN kind = 'usage' THEN 0 ELSE 1 END, occurred_at ASC, id ASC",
      limit.toString(),
    ).use { cursor ->
      while (cursor.moveToNext()) {
        events += JournalEvent(
          id = cursor.getString(0),
          kind = cursor.getString(1),
          packageName = cursor.getString(2),
          displayName = cursor.getString(3),
          startedAtEpochMs = cursor.getLong(4),
          elapsedSeconds = cursor.getInt(5),
          localDate = cursor.getString(6),
          occurredAtEpochMs = cursor.getLong(7),
          reason = cursor.getString(8),
          ruleId = cursor.getString(9),
          runId = cursor.getString(10),
          phase = cursor.getString(11),
          vaultId = cursor.getString(12),
        )
      }
    }
    return events
  }

  fun acknowledge(ids: List<String>) {
    if (ids.isEmpty()) return
    writableDatabase.beginTransaction()
    try {
      for (id in ids.distinct().take(500)) {
        writableDatabase.delete("journal_events", "id = ?", arrayOf(id))
      }
      writableDatabase.setTransactionSuccessful()
    } finally {
      writableDatabase.endTransaction()
    }
  }

  fun metadata(key: String): Long? = readableDatabase.query(
    "metadata",
    arrayOf("value"),
    "key = ?",
    arrayOf(key),
    null,
    null,
    null,
    "1",
  ).use { cursor -> cursor.takeIf { it.moveToFirst() }?.getString(0)?.toLongOrNull() }

  private fun writeMetadata(key: String, value: Long) {
    writableDatabase.insertWithOnConflict(
      "metadata",
      null,
      ContentValues().apply {
        put("key", key)
        put("value", value.toString())
      },
      SQLiteDatabase.CONFLICT_REPLACE,
    )
  }

  fun clearTotalsAndCheckpoints() {
    writableDatabase.beginTransaction()
    try {
      writableDatabase.delete("daily_totals", null, null)
      writableDatabase.delete("metadata", null, null)
      writableDatabase.setTransactionSuccessful()
    } finally {
      writableDatabase.endTransaction()
    }
  }

  private fun pendingCount(): Int = readableDatabase.rawQuery("SELECT COUNT(*) FROM journal_events", null)
    .use { cursor -> check(cursor.moveToFirst()); cursor.getInt(0) }

  /** Compact only unsent identities inside the caller's evidence/checkpoint transaction. */
  private fun compactInTransaction() {
    check(writableDatabase.inTransaction()) { "Distractions compaction requires a transaction" }
    if (pendingCount() <= MAX_PENDING_EVENTS) return
    val groups = mutableListOf<JournalEvent>()
    readableDatabase.rawQuery(
      """
        SELECT package_name, MAX(display_name), MIN(started_at), SUM(elapsed_seconds),
               local_date, MAX(occurred_at), vault_id
        FROM journal_events
        WHERE kind = 'usage' AND exported = 0
        GROUP BY package_name, local_date, vault_id
      """.trimIndent(),
      null,
    ).use { cursor ->
      while (cursor.moveToNext()) {
        val base = JournalEvent(
          id = "compact-${UUID.randomUUID()}",
          kind = "usage",
          packageName = cursor.getString(0),
          displayName = cursor.getString(1),
          startedAtEpochMs = cursor.getLong(2),
          elapsedSeconds = 0,
          localDate = cursor.getString(4),
          occurredAtEpochMs = cursor.getLong(5),
          reason = null,
          ruleId = null,
          runId = null,
          phase = null,
          vaultId = cursor.getString(6),
        )
        for (seconds in compactedUsageParts(cursor.getLong(3))) {
          groups += base.copy(id = "compact-${UUID.randomUUID()}", elapsedSeconds = seconds)
        }
      }
    }
    writableDatabase.execSQL(
      """
        DELETE FROM journal_events
        WHERE kind = 'usage' AND exported = 0
      """.trimIndent(),
    )
    groups.forEach(::insertCompactedEvent)
    writableDatabase.execSQL(
      """
        DELETE FROM journal_events WHERE kind = 'block' AND id NOT IN (
          SELECT id FROM journal_events WHERE kind = 'block' ORDER BY occurred_at DESC LIMIT 500
        ) AND exported = 0
      """.trimIndent(),
    )
  }

  private fun insertCompactedEvent(event: JournalEvent) {
    writableDatabase.insertOrThrow("journal_events", null, ContentValues().apply {
      put("id", event.id)
      put("kind", event.kind)
      put("package_name", event.packageName)
      put("display_name", event.displayName)
      put("started_at", event.startedAtEpochMs)
      put("elapsed_seconds", event.elapsedSeconds)
      put("local_date", event.localDate)
      put("occurred_at", event.occurredAtEpochMs)
      put("vault_id", event.vaultId)
    })
  }

  private fun pruneDailyTotals(nowEpochMs: Long) {
    val cutoff = Instant.ofEpochMilli(nowEpochMs).atZone(ZoneId.systemDefault())
      .toLocalDate().minusDays(14).toString()
    writableDatabase.delete("daily_totals", "local_date < ?", arrayOf(cutoff))
  }

  private fun incrementDailyTotal(
    packageName: String,
    localDate: String,
    elapsedSeconds: Int,
  ) {
    val inserted = writableDatabase.insertWithOnConflict(
      "daily_totals",
      null,
      ContentValues().apply {
        put("package_name", packageName)
        put("local_date", localDate)
        put("elapsed_seconds", elapsedSeconds)
      },
      SQLiteDatabase.CONFLICT_IGNORE,
    )
    if (inserted != -1L) return

    writableDatabase.compileStatement(
      """
      UPDATE daily_totals
      SET elapsed_seconds = elapsed_seconds + ?
      WHERE package_name = ? AND local_date = ?
      """.trimIndent(),
    ).use { statement ->
      statement.bindLong(1, elapsedSeconds.toLong())
      statement.bindString(2, packageName)
      statement.bindString(3, localDate)
      check(statement.executeUpdateDelete() == 1) {
        "Android usage total could not be updated"
      }
    }
  }

  private data class UsageSlice(
    val startEpochMs: Long,
    val endEpochMs: Long,
    val localDate: String,
  )

  private fun splitByLocalDate(startedAtEpochMs: Long, endedAtEpochMs: Long): List<UsageSlice> {
    val zone = ZoneId.systemDefault()
    val slices = mutableListOf<UsageSlice>()
    var cursor = startedAtEpochMs
    while (cursor < endedAtEpochMs) {
      val day = Instant.ofEpochMilli(cursor).atZone(zone).toLocalDate()
      val nextDay = day.plusDays(1).atStartOfDay(zone).toInstant().toEpochMilli()
      val end = minOf(endedAtEpochMs, nextDay)
      slices += UsageSlice(cursor, end, day.toString())
      require(slices.size <= MAX_PENDING_EVENTS) { "Distractions usage interval spans too many dates" }
      cursor = end
    }
    return slices
  }

  private fun localDate(epochMs: Long): String = Instant.ofEpochMilli(epochMs)
    .atZone(ZoneId.systemDefault()).toLocalDate().toString()

  companion object {
    private const val MAX_PENDING_EVENTS = 2_000
    private const val MAX_ACCOUNTING_SOURCES = 10_000
    private val EVENT_COLUMNS = arrayOf(
      "id",
      "kind",
      "package_name",
      "display_name",
      "started_at",
      "elapsed_seconds",
      "local_date",
      "occurred_at",
      "reason",
      "rule_id",
      "run_id",
      "phase",
      "vault_id",
    )
  }
}

/** Splits a usage total into parts of at most one day so compacted evidence and days longer than 24 hours keep every second. */
internal fun compactedUsageParts(totalSeconds: Long): List<Int> {
  require(totalSeconds >= 0 && totalSeconds <= 2_000L * 86_400L) {
    "Compacted Distractions usage is outside its bound"
  }
  val result = mutableListOf<Int>()
  var remaining = totalSeconds
  while (remaining > 0) {
    val part = minOf(remaining, 86_400L).toInt()
    result += part
    remaining -= part
  }
  return result
}

/** Reject overflow before the surrounding transaction can commit a checkpoint. */
internal fun requireJournalCapacity(pendingCount: Int, maximum: Int) {
  require(pendingCount in 0..maximum) {
    "Distractions journal is full; synchronize retained events before recording more usage"
  }
}
