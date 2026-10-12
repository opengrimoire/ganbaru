package org.opengrimoire.ganbaruai.mobile.distractions

import org.json.JSONObject
import java.time.LocalDate

internal data class MobileAppRule(
  val name: String,
  val packageName: String,
  val enabled: Boolean,
)

internal data class MobileSchedule(
  val enabled: Boolean,
  val blockDuringFocus: Boolean,
  val blockDuringShortBreaks: Boolean,
  val blockDuringLongBreaks: Boolean,
  val pauseDuringFocusPause: Boolean,
  val blockedApps: List<MobileAppRule>,
)

internal data class MobileLimit(
  val id: String,
  val name: String,
  val enabled: Boolean,
  val minutesPerDay: Int?,
  val minutesPerWeek: Int?,
  val packages: Set<String>,
  val acceptedDailyUsage: AcceptedUsage? = null,
  val acceptedWeeklyUsage: AcceptedUsage? = null,
)

internal data class AcceptedUsage(
  val windowStartLocalDate: String,
  val windowEndLocalDate: String,
  val usedSeconds: Long,
  val localUsedSecondsAtCapture: Long? = null,
)

/** Count observations after capture even when publication arrives later. */
internal fun combinedUsageSinceAcceptance(
  acceptedUsedSeconds: Long,
  localUsedSecondsAtAcceptance: Long,
  currentLocalUsedSeconds: Long,
): Long {
  require(acceptedUsedSeconds >= 0 && localUsedSecondsAtAcceptance >= 0 && currentLocalUsedSeconds >= 0) {
    "Distractions usage counters must be nonnegative"
  }
  return Math.addExact(acceptedUsedSeconds,
    (currentLocalUsedSeconds - localUsedSecondsAtAcceptance).coerceAtLeast(0L))
}

internal data class DistractionsCopy(
  val channelName: String,
  val channelDescription: String,
  val blockedMessage: String,
  val limitMessage: String,
)

internal data class DistractionsRulesSnapshot(
  val vaultId: String,
  val revision: String,
  val generatedAtEpochMs: Long,
  val mobile: MobileSchedule,
  val limitsEnabled: Boolean,
  val limits: List<MobileLimit>,
  val copy: DistractionsCopy,
)

internal data class PomodoroPhaseState(
  val active: Boolean,
  val runId: String?,
  val phase: String?,
  val running: Boolean,
  val validUntilEpochMs: Long,
)

internal data class BlockDecision(
  val blocked: Boolean,
  val reason: String? = null,
  val ruleId: String? = null,
)

internal object DistractionsRuleCodec {
  private const val MAX_RULES = 256
  private const val MAX_LIMITS = 128
  private const val MAX_SNAPSHOT_BYTES = 512 * 1024

  fun decode(encoded: String): DistractionsRulesSnapshot {
    require(encoded.toByteArray().size in 2..MAX_SNAPSHOT_BYTES) {
      "Distractions rule snapshot is outside its size limit"
    }
    val root = JSONObject(encoded)
    require(root.getInt("schemaVersion") == 1) { "Unsupported Distractions rule schema" }
    val revision = bounded(root.getString("revision"), 1, 120, "revision")
    val generatedAt = root.getLong("generatedAtEpochMs")
    require(generatedAt > 0L) { "Distractions snapshot timestamp is invalid" }

    val mobileJson = root.getJSONObject("mobile")
    val blockedAppsJson = mobileJson.getJSONArray("blockedApps")
    require(blockedAppsJson.length() <= MAX_RULES) { "Too many blocked mobile apps" }
    val blockedApps = (0 until blockedAppsJson.length()).map { index ->
      val item = blockedAppsJson.getJSONObject(index)
      MobileAppRule(
        name = bounded(item.getString("name"), 1, 120, "app name"),
        packageName = packageName(item.getString("packageName")),
        enabled = item.getBoolean("enabled"),
      )
    }.distinctBy { it.packageName.lowercase() }

    val limitsJson = root.getJSONObject("limits")
    val itemsJson = limitsJson.getJSONArray("items")
    require(itemsJson.length() <= MAX_LIMITS) { "Too many mobile usage limits" }
    val limits = (0 until itemsJson.length()).map { index ->
      val item = itemsJson.getJSONObject(index)
      val packagesJson = item.getJSONArray("packages")
      require(packagesJson.length() <= MAX_RULES) { "Too many packages in a usage limit" }
      val packages = (0 until packagesJson.length())
        .map { packageIndex -> packageName(packagesJson.getString(packageIndex)).lowercase() }
        .toSet()
      MobileLimit(
        id = bounded(item.getString("id"), 1, 80, "limit ID"),
        name = bounded(item.getString("name"), 1, 80, "limit name"),
        enabled = item.getBoolean("enabled"),
        minutesPerDay = optionalMinutes(item, "minutesPerDay", 24 * 60),
        minutesPerWeek = optionalMinutes(item, "minutesPerWeek", 7 * 24 * 60),
        packages = packages,
        acceptedDailyUsage = acceptedUsage(item, "day"),
        acceptedWeeklyUsage = acceptedUsage(item, "week"),
      )
    }.filter { it.packages.isNotEmpty() }

    val copyJson = root.getJSONObject("copy")
    return DistractionsRulesSnapshot(
      vaultId = bounded(root.getString("vaultId"), 1, 128, "vault ID"),
      revision = revision,
      generatedAtEpochMs = generatedAt,
      mobile = MobileSchedule(
        enabled = mobileJson.getBoolean("enabled"),
        blockDuringFocus = mobileJson.getBoolean("blockDuringFocus"),
        blockDuringShortBreaks = mobileJson.getBoolean("blockDuringShortBreaks"),
        blockDuringLongBreaks = mobileJson.getBoolean("blockDuringLongBreaks"),
        pauseDuringFocusPause = mobileJson.getBoolean("pauseDuringFocusPause"),
        blockedApps = blockedApps,
      ),
      limitsEnabled = limitsJson.getBoolean("enabled"),
      limits = limits,
      copy = DistractionsCopy(
        channelName = bounded(copyJson.getString("channelName"), 1, 80, "channel name"),
        channelDescription = bounded(
          copyJson.getString("channelDescription"),
          1,
          160,
          "channel description",
        ),
        blockedMessage = bounded(copyJson.getString("blockedMessage"), 1, 120, "blocked message"),
        limitMessage = bounded(copyJson.getString("limitMessage"), 1, 120, "limit message"),
      ),
    )
  }

  private fun optionalMinutes(value: JSONObject, key: String, maximum: Int): Int? {
    require(value.has(key)) { "$key is required" }
    if (value.isNull(key)) return null
    return value.getInt(key).also { require(it in 1..maximum) { "$key is invalid" } }
  }

  private fun acceptedUsage(value: JSONObject, period: String): AcceptedUsage? {
    val accepted = value.optJSONObject("acceptedUsage")?.optJSONObject(period) ?: return null
    val start = accepted.getString("windowStartLocalDate")
    val end = accepted.getString("windowEndLocalDate")
    require(runCatching { LocalDate.parse(start) }.isSuccess &&
      runCatching { LocalDate.parse(end) }.isSuccess && start <= end) {
      "Accepted Distractions usage window is invalid"
    }
    val usedSeconds = accepted.getLong("usedSeconds")
    require(usedSeconds >= 0) { "Accepted Distractions usage is invalid" }
    val captured = if (accepted.has("localUsedSecondsAtCapture")) {
      accepted.getLong("localUsedSecondsAtCapture").also {
        require(it >= 0) { "Captured local Distractions usage is invalid" }
      }
    } else null
    return AcceptedUsage(start, end, usedSeconds, captured)
  }

  private fun bounded(value: String, minimum: Int, maximum: Int, label: String): String =
    value.trim().also {
      require(it.codePointCount(0, it.length) in minimum..maximum) { "$label is invalid" }
    }

  private fun packageName(value: String): String = value.trim().also {
    require(it.length in 3..255 && PACKAGE_PATTERN.matches(it)) {
      "Android package name is invalid"
    }
  }

  private val PACKAGE_PATTERN = Regex("^[A-Za-z][A-Za-z0-9_]*(?:\\.[A-Za-z][A-Za-z0-9_]*)+$")
}

internal object DistractionsEvaluator {
  fun isBlockedBySchedule(
    schedule: MobileSchedule,
    phase: PomodoroPhaseState?,
    packageName: String,
    nowEpochMs: Long,
  ): Boolean {
    if (!schedule.enabled) return false
    val rule = schedule.blockedApps.firstOrNull {
      it.enabled && it.packageName.equals(packageName, ignoreCase = true)
    } ?: return false
    if (rule.packageName.isBlank() || phase == null || !phase.active) return false
    if (phase.validUntilEpochMs <= nowEpochMs) return false
    if (!phase.running && schedule.pauseDuringFocusPause) return false
    return when (phase.phase) {
      "focus" -> schedule.blockDuringFocus
      "short_break" -> schedule.blockDuringShortBreaks
      "long_break" -> schedule.blockDuringLongBreaks
      else -> false
    }
  }
}
