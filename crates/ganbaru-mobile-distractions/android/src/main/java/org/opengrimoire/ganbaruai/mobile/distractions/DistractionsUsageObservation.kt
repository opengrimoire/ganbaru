package org.opengrimoire.ganbaruai.mobile.distractions

internal enum class UsageObservationEventKind {
  ACTIVITY_RESUMED,
  ACTIVITY_PAUSED,
  ACTIVITY_STOPPED,
  ACTIVITY_VISIBLE_ROLLOVER,
  SCREEN_INTERACTIVE,
  SCREEN_NON_INTERACTIVE,
  KEYGUARD_SHOWN,
  KEYGUARD_HIDDEN,
  DEVICE_SHUTDOWN,
  DEVICE_STARTUP,
}

internal data class UsageObservationEvent(
  val packageName: String?,
  val activityName: String?,
  val kind: UsageObservationEventKind,
  val timestampEpochMs: Long,
)

internal data class VisibleUsageInterval(
  val packageName: String,
  val startedAtEpochMs: Long,
  val endedAtEpochMs: Long,
)

internal data class UsageObservationState(
  val visibleActivities: Map<String, Set<String>> = emptyMap(),
  val screenInteractive: Boolean = true,
  val keyguardVisible: Boolean = false,
) {
  fun withDeviceState(
    screenInteractive: Boolean,
    keyguardVisible: Boolean,
  ): UsageObservationState = copy(
    screenInteractive = screenInteractive,
    keyguardVisible = keyguardVisible,
  )
}

internal data class UsageObservationResult(
  val state: UsageObservationState,
  val intervals: List<VisibleUsageInterval>,
  val visiblePackages: Set<String>,
)

internal object DistractionsUsageObserver {
  fun reconcile(
    initialState: UsageObservationState,
    events: List<UsageObservationEvent>,
    observedPackages: Set<String>,
    usagePackages: Set<String>,
    intervalStartEpochMs: Long,
    intervalEndEpochMs: Long,
  ): UsageObservationResult {
    val normalizedObservedPackages = observedPackages.mapTo(mutableSetOf(), String::lowercase)
    val normalizedUsagePackages = usagePackages.mapTo(mutableSetOf(), String::lowercase)
    val visibleActivities = initialState.visibleActivities
      .filterKeys { it in normalizedObservedPackages }
      .mapValuesTo(mutableMapOf()) { (_, activities) -> activities.toMutableSet() }
    var screenInteractive = initialState.screenInteractive
    var keyguardVisible = initialState.keyguardVisible
    var cursor = intervalStartEpochMs.coerceAtMost(intervalEndEpochMs)
    val intervals = mutableMapOf<String, MutableList<VisibleUsageInterval>>()

    fun recordVisibleUntil(endedAtEpochMs: Long) {
      val end = endedAtEpochMs.coerceIn(cursor, intervalEndEpochMs)
      if (end <= cursor || !screenInteractive || keyguardVisible) {
        cursor = end
        return
      }
      visibleActivities.keys
        .asSequence()
        .filter { it in normalizedUsagePackages }
        .forEach { packageName ->
          val packageIntervals = intervals.getOrPut(packageName) { mutableListOf() }
          val previous = packageIntervals.lastOrNull()
          if (previous != null && previous.endedAtEpochMs == cursor) {
            packageIntervals[packageIntervals.lastIndex] = previous.copy(endedAtEpochMs = end)
          } else {
            packageIntervals += VisibleUsageInterval(packageName, cursor, end)
          }
        }
      cursor = end
    }

    events.asSequence()
      .filter { it.timestampEpochMs <= intervalEndEpochMs }
      .sortedBy(UsageObservationEvent::timestampEpochMs)
      .forEach { event ->
        if (event.timestampEpochMs > intervalStartEpochMs) {
          recordVisibleUntil(event.timestampEpochMs)
        }
        when (event.kind) {
          UsageObservationEventKind.ACTIVITY_RESUMED,
          UsageObservationEventKind.ACTIVITY_PAUSED,
          -> updateVisibleActivity(visibleActivities, event, normalizedObservedPackages)
          UsageObservationEventKind.ACTIVITY_STOPPED -> removeVisibleActivity(
            visibleActivities,
            event,
            normalizedObservedPackages,
          )
          UsageObservationEventKind.ACTIVITY_VISIBLE_ROLLOVER -> markPackageVisible(
            visibleActivities,
            event,
            normalizedObservedPackages,
          )
          UsageObservationEventKind.SCREEN_INTERACTIVE -> screenInteractive = true
          UsageObservationEventKind.SCREEN_NON_INTERACTIVE -> screenInteractive = false
          UsageObservationEventKind.KEYGUARD_SHOWN -> keyguardVisible = true
          UsageObservationEventKind.KEYGUARD_HIDDEN -> keyguardVisible = false
          UsageObservationEventKind.DEVICE_SHUTDOWN,
          UsageObservationEventKind.DEVICE_STARTUP,
          -> {
            visibleActivities.clear()
            screenInteractive = false
            keyguardVisible = true
          }
        }
      }
    recordVisibleUntil(intervalEndEpochMs)

    val immutableActivities = visibleActivities
      .filterValues(Set<String>::isNotEmpty)
      .mapValues { (_, activities) -> activities.toSet() }
    return UsageObservationResult(
      state = UsageObservationState(
        visibleActivities = immutableActivities,
        screenInteractive = screenInteractive,
        keyguardVisible = keyguardVisible,
      ),
      intervals = intervals.values.flatten().sortedWith(
        compareBy(VisibleUsageInterval::startedAtEpochMs, VisibleUsageInterval::packageName),
      ),
      visiblePackages = immutableActivities.keys,
    )
  }

  private fun updateVisibleActivity(
    visibleActivities: MutableMap<String, MutableSet<String>>,
    event: UsageObservationEvent,
    observedPackages: Set<String>,
  ) {
    val packageName = event.packageName?.lowercase() ?: return
    if (packageName !in observedPackages) return
    val activities = visibleActivities.getOrPut(packageName) { mutableSetOf() }
    if (!event.activityName.isNullOrBlank()) activities.remove(packageName)
    activities.add(activityKey(packageName, event.activityName))
  }

  private fun removeVisibleActivity(
    visibleActivities: MutableMap<String, MutableSet<String>>,
    event: UsageObservationEvent,
    observedPackages: Set<String>,
  ) {
    val packageName = event.packageName?.lowercase() ?: return
    if (packageName !in observedPackages) return
    val activities = visibleActivities[packageName] ?: return
    activities.remove(packageName)
    val activityName = event.activityName
    if (activityName.isNullOrBlank()) {
      activities.clear()
    } else {
      activities.remove(activityKey(packageName, activityName))
    }
    if (activities.isEmpty()) visibleActivities.remove(packageName)
  }

  private fun markPackageVisible(
    visibleActivities: MutableMap<String, MutableSet<String>>,
    event: UsageObservationEvent,
    observedPackages: Set<String>,
  ) {
    val packageName = event.packageName?.lowercase() ?: return
    if (packageName !in observedPackages) return
    visibleActivities[packageName] = mutableSetOf(packageName)
  }

  private fun activityKey(packageName: String, activityName: String?): String =
    activityName?.takeIf(String::isNotBlank) ?: packageName
}
