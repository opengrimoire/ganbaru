package org.opengrimoire.ganbaruai.mobile.distractions

import android.app.AppOpsManager
import android.app.KeyguardManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.usage.UsageEvents
import android.app.usage.UsageStatsManager
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Process
import android.provider.Settings
import android.telecom.TelecomManager
import android.util.Log
import android.view.accessibility.AccessibilityManager
import java.time.Instant
import java.time.ZoneId
import org.json.JSONArray
import org.json.JSONObject

internal const val ACTION_DISTRACTIONS_PHASE = "app.ganbaru.intent.action.DISTRACTIONS_PHASE"
internal const val ACTION_DISTRACTIONS_PHASE_CLEAR =
  "app.ganbaru.intent.action.DISTRACTIONS_PHASE_CLEAR"
internal const val ACTION_DISTRACTIONS_RULES_CHANGED =
  "app.ganbaru.intent.action.DISTRACTIONS_RULES_CHANGED"
private const val RUNTIME_STORE = "GANBARU_DISTRACTIONS_RUNTIME"
private const val RULES_KEY = "rules"
private const val RULES_VAULT_ID_KEY = "rulesVaultId"
private const val NOTIFICATION_COPY_KEY = "notificationCopy"
private const val PHASE_ACTIVE_KEY = "phaseActive"
private const val PHASE_RUN_ID_KEY = "phaseRunId"
private const val PHASE_KIND_KEY = "phaseKind"
private const val PHASE_RUNNING_KEY = "phaseRunning"
private const val PHASE_VALID_UNTIL_KEY = "phaseValidUntil"
private const val NOTIFICATION_TIMESTAMPS_KEY = "notificationTimestamps"
private const val COMBINED_USAGE_BASELINES_KEY = "combinedUsageBaselines"
private const val BLOCK_CHANNEL_ID = "distractions-blocks-v1"
private const val BLOCK_NOTIFICATION_BASE_ID = 1_500_100_000

internal object DistractionsRuntimeStore {
  fun saveRules(context: Context, encoded: String) {
    val next = DistractionsRuleCodec.decode(encoded)
    val prefs = preferences(context)
    val previousVaultId = prefs.getString(RULES_VAULT_ID_KEY, null)
    val incompletePreviousRules = prefs.contains(RULES_KEY) && previousVaultId == null
    if (incompletePreviousRules || (previousVaultId != null && previousVaultId != next.vaultId)) {
      DistractionsJournal(context).use { it.clearTotalsAndCheckpoints() }
    }
    val baselines = JSONObject().apply {
      put("revision", next.revision)
      put("items", JSONArray().apply {
        DistractionsJournal(context).use { journal ->
          for (limit in next.limits) {
            for ((period, accepted) in listOf(
              "day" to limit.acceptedDailyUsage,
              "week" to limit.acceptedWeeklyUsage,
            )) {
              if (accepted == null) continue
              put(JSONObject().apply {
                put("limitId", limit.id)
                put("period", period)
                put("windowStartLocalDate", accepted.windowStartLocalDate)
                put("windowEndLocalDate", accepted.windowEndLocalDate)
                put("acceptedUsedSeconds", accepted.usedSeconds)
                put("localUsedSeconds", accepted.localUsedSecondsAtCapture ?: journal.usedSeconds(
                  limit.packages,
                  accepted.windowStartLocalDate,
                  accepted.windowEndLocalDate,
                ))
              })
            }
          }
        }
      })
    }
    check(prefs.edit()
      .putString(RULES_KEY, encoded)
      .putString(RULES_VAULT_ID_KEY, next.vaultId)
      .putString(NOTIFICATION_COPY_KEY, JSONObject(mapOf(
        "channelName" to next.copy.channelName, "channelDescription" to next.copy.channelDescription,
        "blockedMessage" to next.copy.blockedMessage, "limitMessage" to next.copy.limitMessage,
      )).toString())
      .putString(COMBINED_USAGE_BASELINES_KEY, baselines.toString())
      .commit()) { "Distractions rule snapshot could not be persisted" }
  }

  /** Revoke enforcement while retaining vault attribution and localized text for reactivation. */
  fun invalidateRules(context: Context) {
    val prefs = preferences(context)
    val retainedCopy = notificationCopy(context)
    val edit = prefs.edit().remove(RULES_KEY).remove(COMBINED_USAGE_BASELINES_KEY)
    if (retainedCopy != null) edit.putString(NOTIFICATION_COPY_KEY, retainedCopy.toString())
    check(edit.commit()) {
      "Distractions rules could not be invalidated"
    }
  }

  fun notificationCopy(context: Context): JSONObject? {
    val prefs = preferences(context)
    val encoded = prefs.getString(NOTIFICATION_COPY_KEY, null)
    if (encoded != null) {
      try {
        val copy = JSONObject(encoded)
        val validated = JSONObject()
        for ((key, maximum) in listOf(
          "channelName" to 80, "channelDescription" to 160,
          "blockedMessage" to 120, "limitMessage" to 120,
        )) {
          val text = copy.getString(key)
          require(text.isNotBlank() && text.codePointCount(0, text.length) <= maximum) {
            "Invalid retained Distractions notification text"
          }
          validated.put(key, text)
        }
        return validated
      } catch (error: Exception) {
        Log.w("DistractionsRuntime", "Discarding invalid retained notification text", error)
        check(prefs.edit().remove(NOTIFICATION_COPY_KEY).commit()) {
          "Invalid Distractions notification text could not be cleared"
        }
      }
    }
    return rules(context)?.copy?.let { copy -> JSONObject(mapOf(
      "channelName" to copy.channelName, "channelDescription" to copy.channelDescription,
      "blockedMessage" to copy.blockedMessage, "limitMessage" to copy.limitMessage,
    )) }
  }

  fun journalVaultId(context: Context): String? = preferences(context).getString(RULES_VAULT_ID_KEY, null)

  fun rules(context: Context): DistractionsRulesSnapshot? {
    val encoded = preferences(context).getString(RULES_KEY, null) ?: return null
    return try {
      DistractionsRuleCodec.decode(encoded)
    } catch (_: Exception) {
      null
    }
  }

  fun combinedUsedSeconds(
    context: Context,
    rules: DistractionsRulesSnapshot,
    limit: MobileLimit,
    period: String,
    accepted: AcceptedUsage,
    currentLocalUsedSeconds: Long,
  ): Long {
    val encoded = preferences(context).getString(COMBINED_USAGE_BASELINES_KEY, null)
      ?: return currentLocalUsedSeconds
    val baseline = runCatching {
      val root = JSONObject(encoded)
      if (root.getString("revision") != rules.revision) return@runCatching null
      val items = root.getJSONArray("items")
      (0 until items.length()).asSequence()
        .map(items::getJSONObject)
        .firstOrNull {
          it.getString("limitId") == limit.id &&
            it.getString("period") == period &&
            it.getString("windowStartLocalDate") == accepted.windowStartLocalDate &&
            it.getString("windowEndLocalDate") == accepted.windowEndLocalDate
        }
    }.getOrNull() ?: return currentLocalUsedSeconds
    val acceptedUsed = baseline.getLong("acceptedUsedSeconds")
    val localAtAcceptance = baseline.getLong("localUsedSeconds")
    return combinedUsageSinceAcceptance(acceptedUsed, localAtAcceptance, currentLocalUsedSeconds)
  }

  fun savePhase(
    context: Context,
    runId: String,
    phase: String,
    running: Boolean,
    validUntilEpochMs: Long,
  ): Boolean {
    if (runId.isBlank() || runId.length > 128
      || phase !in setOf("focus", "short_break", "long_break")
      || validUntilEpochMs <= 0L
    ) {
      clearPhase(context)
      return false
    }
    check(preferences(context).edit()
      .putBoolean(PHASE_ACTIVE_KEY, true)
      .putString(PHASE_RUN_ID_KEY, runId)
      .putString(PHASE_KIND_KEY, phase)
      .putBoolean(PHASE_RUNNING_KEY, running)
      .putLong(PHASE_VALID_UNTIL_KEY, validUntilEpochMs)
      .commit()) { "Distractions phase state could not be persisted" }
    return true
  }

  fun clearPhase(context: Context) {
    check(preferences(context).edit()
      .remove(PHASE_ACTIVE_KEY)
      .remove(PHASE_RUN_ID_KEY)
      .remove(PHASE_KIND_KEY)
      .remove(PHASE_RUNNING_KEY)
      .remove(PHASE_VALID_UNTIL_KEY)
      .commit()) { "Distractions phase state could not be cleared" }
  }

  fun updatePhaseFromIntent(context: Context, intent: Intent): Boolean {
    if (intent.action == ACTION_DISTRACTIONS_PHASE_CLEAR) {
      clearPhase(context)
      return true
    }
    if (intent.action != ACTION_DISTRACTIONS_PHASE) return false
    return savePhase(
      context = context,
      runId = intent.getStringExtra("runId").orEmpty(),
      phase = intent.getStringExtra("phase").orEmpty(),
      running = intent.getBooleanExtra("running", false),
      validUntilEpochMs = intent.getLongExtra("validUntilEpochMs", 0L),
    )
  }

  fun phase(context: Context): PomodoroPhaseState? {
    val prefs = preferences(context)
    if (!prefs.getBoolean(PHASE_ACTIVE_KEY, false)) return null
    return PomodoroPhaseState(
      active = true,
      runId = prefs.getString(PHASE_RUN_ID_KEY, null),
      phase = prefs.getString(PHASE_KIND_KEY, null),
      running = prefs.getBoolean(PHASE_RUNNING_KEY, false),
      validUntilEpochMs = prefs.getLong(PHASE_VALID_UNTIL_KEY, 0L),
    )
  }

  fun allowNotification(context: Context, nowEpochMs: Long): Boolean {
    val prefs = preferences(context)
    val recent = prefs.getString(NOTIFICATION_TIMESTAMPS_KEY, "")
      .orEmpty()
      .split(',')
      .mapNotNull(String::toLongOrNull)
      .filter { nowEpochMs - it < 60_000L }
    if (recent.size >= 5) return false
    prefs.edit().putString(
      NOTIFICATION_TIMESTAMPS_KEY,
      (recent + nowEpochMs).joinToString(","),
    ).apply()
    return true
  }

  private fun preferences(context: Context) = context.getSharedPreferences(
    RUNTIME_STORE,
    Context.MODE_PRIVATE,
  )
}

class DistractionsPhaseReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent) {
    synchronized(DISTRACTIONS_GUARDIAN_LOCK) {
      DistractionsRuntimeStore.updatePhaseFromIntent(context, intent)
    }
  }
}

internal object DistractionsAccess {
  fun hasUsageAccess(context: Context): Boolean {
    val manager = context.getSystemService(Context.APP_OPS_SERVICE) as AppOpsManager
    return manager.checkOpNoThrow(
      AppOpsManager.OPSTR_GET_USAGE_STATS,
      Process.myUid(),
      context.packageName,
    ) == AppOpsManager.MODE_ALLOWED
  }

  fun hasAccessibilityAccess(context: Context): Boolean {
    val manager = context.getSystemService(Context.ACCESSIBILITY_SERVICE) as AccessibilityManager
    return manager.getEnabledAccessibilityServiceList(
      android.accessibilityservice.AccessibilityServiceInfo.FEEDBACK_ALL_MASK,
    ).any { info ->
      val serviceInfo = info.resolveInfo?.serviceInfo ?: return@any false
      serviceInfo.name == DistractionsAccessibilityService::class.java.name
        || serviceInfo.name.endsWith(".DistractionsAccessibilityService")
    }
  }
}

internal object ProtectedPackages {
  fun resolve(context: Context): Set<String> {
    val manager = context.packageManager
    val packages = mutableSetOf(context.packageName, "android")
    defaultActivityPackage(manager, Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_HOME))?.let(packages::add)
    defaultActivityPackage(manager, Intent(Settings.ACTION_SETTINGS))?.let(packages::add)
    defaultActivityPackage(manager, Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS))?.let(packages::add)
    val telecom = context.getSystemService(Context.TELECOM_SERVICE) as? TelecomManager
    telecom?.defaultDialerPackage?.let(packages::add)
    return packages
  }

  fun isProtected(context: Context, packageName: String): Boolean {
    if (packageName in resolve(context)) return true
    val info = try {
      context.packageManager.getApplicationInfo(packageName, 0)
    } catch (_: PackageManager.NameNotFoundException) {
      return true
    }
    return info.uid < Process.FIRST_APPLICATION_UID
  }

  private fun defaultActivityPackage(manager: PackageManager, intent: Intent): String? =
    manager.resolveActivity(intent, PackageManager.MATCH_DEFAULT_ONLY)?.activityInfo?.packageName
}

internal data class DistractionsRecoveryScope(
  val usagePackages: Set<String>,
  val observedPackages: Set<String>,
)

internal object DistractionsRecovery {
  private const val EVENT_WINDOW_MS = 3 * 24 * 60 * 60 * 1_000L
  private const val EVENT_DELIVERY_OVERLAP_MS = 5_000L

  fun scope(rules: DistractionsRulesSnapshot): DistractionsRecoveryScope {
    val usagePackages = if (rules.limitsEnabled) {
      rules.limits.filter { it.enabled }.flatMapTo(mutableSetOf()) { it.packages }
    } else {
      mutableSetOf()
    }
    val observedPackages = rules.mobile.blockedApps
      .filterTo(mutableListOf()) { it.enabled }
      .mapTo(usagePackages.toMutableSet()) { it.packageName.lowercase() }
    return DistractionsRecoveryScope(usagePackages, observedPackages)
  }

  fun queryStart(nowEpochMs: Long): Long = (nowEpochMs - EVENT_WINDOW_MS).coerceAtLeast(0L)

  fun incrementalQueryStart(observedThroughEpochMs: Long): Long =
    (observedThroughEpochMs - EVENT_DELIVERY_OVERLAP_MS).coerceAtLeast(0L)
}

internal class DistractionsEngine(private val context: Context) {
  private val journal = DistractionsJournal(context)
  private var observationState: UsageObservationState? = null
  private var observationScope: DistractionsRecoveryScope? = null
  private var observedThroughEpochMs: Long = 0L
  private var lastBlockedPackage: String? = null
  private var lastBlockedAtEpochMs: Long = 0L

  fun onAccessibilityPackage(packageName: String, nowEpochMs: Long): Boolean {
    val rules = prepareObservation(nowEpochMs) ?: return false
    if (ProtectedPackages.isProtected(context, packageName)) return false
    val decision = evaluate(rules, packageName, nowEpochMs)
    if (!decision.blocked) return false
    return enforce(packageName, rules, decision, nowEpochMs)
  }

  fun onCheckpoint(nowEpochMs: Long): Boolean {
    val rules = prepareObservation(nowEpochMs) ?: return false
    val visiblePackages = observationState?.visibleActivities?.keys.orEmpty()
    if (!isInteractiveAndUnlocked()) return false
    for (packageName in visiblePackages.sorted()) {
      if (ProtectedPackages.isProtected(context, packageName)) continue
      val decision = evaluate(rules, packageName, nowEpochMs)
      if (decision.blocked) return enforce(packageName, rules, decision, nowEpochMs)
    }
    return false
  }

  private fun enforce(
    packageName: String,
    rules: DistractionsRulesSnapshot,
    decision: BlockDecision,
    nowEpochMs: Long,
  ): Boolean {
    if (lastBlockedPackage == packageName && nowEpochMs - lastBlockedAtEpochMs < 1_500L) return true
    lastBlockedPackage = packageName
    lastBlockedAtEpochMs = nowEpochMs
    val displayName = appLabel(packageName)
    val phase = DistractionsRuntimeStore.phase(context)
    journal.recordBlock(
      packageName,
      displayName,
      nowEpochMs,
      decision.reason ?: "rule",
      decision.ruleId,
      phase?.runId,
      phase?.phase,
      rules.vaultId,
    )
    notifyBlocked(rules, packageName, displayName, decision, nowEpochMs)
    return true
  }

  private fun prepareObservation(nowEpochMs: Long): DistractionsRulesSnapshot? {
    if (!DistractionsAccess.hasUsageAccess(context)) {
      resetObservation()
      return null
    }
    val rules = DistractionsRuntimeStore.rules(context) ?: run {
      resetObservation()
      return null
    }
    val scope = DistractionsRecovery.scope(rules)
    val lastObservedEpochMs = (journal.metadata("lastObservedEpochMs") ?: nowEpochMs)
      .coerceIn(0L, nowEpochMs)
    val mustRebuild = observationState == null
      || observationScope != scope
      || observedThroughEpochMs > nowEpochMs
      || lastObservedEpochMs < observedThroughEpochMs
    val queryStart = if (mustRebuild) {
      DistractionsRecovery.queryStart(nowEpochMs)
    } else {
      DistractionsRecovery.incrementalQueryStart(observedThroughEpochMs)
    }
    val initialState = if (mustRebuild) UsageObservationState() else checkNotNull(observationState)
    val result = if (nowEpochMs > queryStart) {
      DistractionsUsageObserver.reconcile(
        initialState = initialState,
        events = queryUsageEvents(queryStart, nowEpochMs),
        observedPackages = scope.observedPackages,
        usagePackages = scope.usagePackages,
        intervalStartEpochMs = lastObservedEpochMs,
        intervalEndEpochMs = nowEpochMs,
      )
    } else {
      UsageObservationResult(
        state = initialState,
        intervals = emptyList(),
        visiblePackages = initialState.visibleActivities.keys,
      )
    }
    val usageIntervals = result.intervals.map { interval ->
      JournalUsageInterval(
        packageName = interval.packageName,
        displayName = appLabel(interval.packageName),
        startedAtEpochMs = interval.startedAtEpochMs,
        endedAtEpochMs = interval.endedAtEpochMs,
      )
    }
    journal.recordUsageBatch(rules.vaultId, usageIntervals, nowEpochMs)
    val power = context.getSystemService(Context.POWER_SERVICE) as android.os.PowerManager
    val keyguard = context.getSystemService(Context.KEYGUARD_SERVICE) as KeyguardManager
    observationState = result.state.withDeviceState(
      screenInteractive = power.isInteractive,
      keyguardVisible = keyguard.isKeyguardLocked,
    )
    observationScope = scope
    observedThroughEpochMs = nowEpochMs
    return rules
  }

  private fun queryUsageEvents(
    startedAtEpochMs: Long,
    endedAtEpochMs: Long,
  ): List<UsageObservationEvent> {
    val usageEvents = (context.getSystemService(Context.USAGE_STATS_SERVICE) as UsageStatsManager)
      .queryEvents(startedAtEpochMs, endedAtEpochMs)
      ?: error("Android usage events were unavailable")
    val observations = mutableListOf<UsageObservationEvent>()
    val event = UsageEvents.Event()
    while (usageEvents.hasNextEvent()) {
      usageEvents.getNextEvent(event)
      val kind = eventKind(event.eventType) ?: continue
      observations += UsageObservationEvent(
        packageName = event.packageName?.lowercase(),
        activityName = event.className,
        kind = kind,
        timestampEpochMs = event.timeStamp,
      )
    }
    return observations
  }

  private fun eventKind(eventType: Int): UsageObservationEventKind? = when (eventType) {
    UsageEvents.Event.ACTIVITY_RESUMED -> UsageObservationEventKind.ACTIVITY_RESUMED
    UsageEvents.Event.ACTIVITY_PAUSED -> UsageObservationEventKind.ACTIVITY_PAUSED
    UsageEvents.Event.ACTIVITY_STOPPED -> UsageObservationEventKind.ACTIVITY_STOPPED
    USAGE_EVENT_END_OF_DAY,
    USAGE_EVENT_CONTINUE_PREVIOUS_DAY,
    -> UsageObservationEventKind.ACTIVITY_VISIBLE_ROLLOVER
    UsageEvents.Event.SCREEN_INTERACTIVE -> UsageObservationEventKind.SCREEN_INTERACTIVE
    UsageEvents.Event.SCREEN_NON_INTERACTIVE -> UsageObservationEventKind.SCREEN_NON_INTERACTIVE
    UsageEvents.Event.KEYGUARD_SHOWN -> UsageObservationEventKind.KEYGUARD_SHOWN
    UsageEvents.Event.KEYGUARD_HIDDEN -> UsageObservationEventKind.KEYGUARD_HIDDEN
    UsageEvents.Event.DEVICE_SHUTDOWN -> UsageObservationEventKind.DEVICE_SHUTDOWN
    UsageEvents.Event.DEVICE_STARTUP -> UsageObservationEventKind.DEVICE_STARTUP
    else -> null
  }

  private fun resetObservation() {
    observationState = null
    observationScope = null
    observedThroughEpochMs = 0L
  }

  private fun evaluate(
    rules: DistractionsRulesSnapshot,
    packageName: String,
    nowEpochMs: Long,
  ): BlockDecision {
    if (DistractionsEvaluator.isBlockedBySchedule(
        rules.mobile,
        DistractionsRuntimeStore.phase(context),
        packageName,
        nowEpochMs,
      )) {
      return BlockDecision(true, "schedule", null)
    }
    if (!rules.limitsEnabled) return BlockDecision(false)
    val localDate = localDate(nowEpochMs)
    val weekStart = Instant.ofEpochMilli(nowEpochMs).atZone(ZoneId.systemDefault())
      .toLocalDate().minusDays((Instant.ofEpochMilli(nowEpochMs).atZone(ZoneId.systemDefault())
        .dayOfWeek.value - 1).toLong()).toString()
    for (limit in rules.limits) {
      if (!limit.enabled || packageName.lowercase() !in limit.packages) continue
      val dailyExhausted = limit.minutesPerDay?.let {
        val localUsed = journal.usedSeconds(limit.packages, localDate, localDate)
        val combinedUsed = limit.acceptedDailyUsage
          ?.takeIf { accepted ->
            accepted.windowStartLocalDate == localDate && accepted.windowEndLocalDate == localDate
          }
          ?.let { accepted ->
            DistractionsRuntimeStore.combinedUsedSeconds(
              context,
              rules,
              limit,
              "day",
              accepted,
              localUsed,
            )
          } ?: localUsed
        combinedUsed >= it * 60L
      } ?: false
      val weeklyExhausted = limit.minutesPerWeek?.let {
        val localUsed = journal.usedSeconds(limit.packages, weekStart, localDate)
        val combinedUsed = limit.acceptedWeeklyUsage
          ?.takeIf { accepted ->
            accepted.windowStartLocalDate == weekStart && accepted.windowEndLocalDate == localDate
          }
          ?.let { accepted ->
            DistractionsRuntimeStore.combinedUsedSeconds(
              context,
              rules,
              limit,
              "week",
              accepted,
              localUsed,
            )
          } ?: localUsed
        combinedUsed >= it * 60L
      } ?: false
      if (dailyExhausted || weeklyExhausted) {
        return BlockDecision(true, "usage_limit", limit.id)
      }
    }
    return BlockDecision(false)
  }

  private fun notifyBlocked(
    rules: DistractionsRulesSnapshot,
    packageName: String,
    displayName: String,
    decision: BlockDecision,
    nowEpochMs: Long,
  ) {
    if (!DistractionsRuntimeStore.allowNotification(context, nowEpochMs)) return
    val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    manager.createNotificationChannel(
      NotificationChannel(
        BLOCK_CHANNEL_ID,
        rules.copy.channelName,
        NotificationManager.IMPORTANCE_DEFAULT,
      ).apply { description = rules.copy.channelDescription },
    )
    val target = if (decision.reason == "usage_limit") "limits" else "mobile"
    val launchIntent = context.packageManager.getLaunchIntentForPackage(context.packageName)?.apply {
      flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
      putExtra(DISTRACTIONS_NOTIFICATION_ACTION_KEY, target)
    }
    val contentIntent = launchIntent?.let {
      PendingIntent.getActivity(
        context,
        BLOCK_NOTIFICATION_BASE_ID + packageName.hashCode(),
        it,
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
      )
    }
    val body = if (decision.reason == "usage_limit") {
      rules.copy.limitMessage
    } else {
      rules.copy.blockedMessage
    }
    manager.notify(
      BLOCK_NOTIFICATION_BASE_ID + (packageName.hashCode() and 0xFFFF),
      Notification.Builder(context, BLOCK_CHANNEL_ID)
        .setSmallIcon(notificationIcon())
        .setContentTitle(displayName)
        .setContentText(body)
        .setCategory(Notification.CATEGORY_STATUS)
        .setVisibility(Notification.VISIBILITY_PRIVATE)
        .setContentIntent(contentIntent)
        .setAutoCancel(true)
        .build(),
    )
  }

  private fun isInteractiveAndUnlocked(): Boolean {
    val power = context.getSystemService(Context.POWER_SERVICE) as android.os.PowerManager
    val keyguard = context.getSystemService(Context.KEYGUARD_SERVICE) as KeyguardManager
    return power.isInteractive && !keyguard.isKeyguardLocked
  }

  private fun appLabel(packageName: String): String = try {
    val info = context.packageManager.getApplicationInfo(packageName, 0)
    context.packageManager.getApplicationLabel(info).toString().trim().take(120)
      .ifBlank { packageName }
  } catch (_: PackageManager.NameNotFoundException) {
    packageName
  }

  private fun localDate(epochMs: Long): String = Instant.ofEpochMilli(epochMs)
    .atZone(ZoneId.systemDefault()).toLocalDate().toString()

  private fun notificationIcon(): Int = context.resources.getIdentifier(
    "ic_notification_focus",
    "drawable",
    context.packageName,
  ).takeIf { it != 0 } ?: context.applicationInfo.icon

  private companion object {
    const val USAGE_EVENT_END_OF_DAY = 3
    const val USAGE_EVENT_CONTINUE_PREVIOUS_DAY = 4
  }
}
