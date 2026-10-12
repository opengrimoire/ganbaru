package org.opengrimoire.ganbaruai.mobile.notifications

import android.app.Activity
import android.app.ActivityManager
import android.app.AlarmManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.media.AudioAttributes
import android.media.RingtoneManager
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import android.webkit.WebView
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Channel
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
internal class CalendarChannelArgs {
  lateinit var name: String
  lateinit var description: String
}

@InvokeArg
internal class CalendarNotificationArgs {
  var id: Int = 0
  lateinit var title: String
  var body: String = ""
  lateinit var eventId: String
  var scheduledAtEpochMs: Long = 0
}

@InvokeArg
internal class CalendarNotificationBatchArgs {
  var notifications: List<CalendarNotificationArgs> = listOf()
}

@InvokeArg
internal class CalendarNotificationCancelArgs {
  var ids: List<Int> = listOf()
}

internal class PomodoroNotificationPhaseArgs {
  lateinit var id: String
  lateinit var phase: String
  var rhythmPosition: Int = 0
  var startsAtEpochMs: Long = 0
  var endsAtEpochMs: Long = 0
}

internal class PomodoroNotificationCopyArgs {
  lateinit var channelName: String
  lateinit var channelDescription: String
  lateinit var alertsChannelName: String
  lateinit var alertsChannelDescription: String
  lateinit var focusTitle: String
  lateinit var shortBreakTitle: String
  lateinit var longBreakTitle: String
  lateinit var pausedText: String
  lateinit var focusCompleteTitle: String
  lateinit var breakCompleteTitle: String
  lateinit var sessionCompleteText: String
}

internal class PomodoroNotificationStateArgs {
  lateinit var runId: String
  lateinit var eventId: String
  var eventTitle: String? = null
  lateinit var eventDate: String
  var eventEndsAtEpochMs: Long = 0
  var generatedAtEpochMs: Long = 0
  var isRunning: Boolean = false
  var remainingSeconds: Int = 0
  var totalSeconds: Int = 0
  lateinit var configJson: String
  var phases: List<PomodoroNotificationPhaseArgs> = listOf()
  lateinit var copy: PomodoroNotificationCopyArgs
}

@InvokeArg
internal class PomodoroNotificationUpdateArgs {
  lateinit var state: PomodoroNotificationStateArgs
  var processNonce: Long = 0
  var generation: Long = 0
  var revision: Long = -1
}

@InvokeArg
internal class FocusNotificationProcessArgs {
  var processNonce: Long = 0
}

@InvokeArg
internal class FocusNotificationCopyArgs {
  lateinit var copy: PomodoroNotificationCopyArgs
  var processNonce: Long = 0
}

internal class PomodoroReminderArgs {
  lateinit var id: String
  lateinit var eventId: String
  var startsAtEpochMs: Long = 0
  var endsAtEpochMs: Long = 0
  lateinit var title: String
  lateinit var body: String
  lateinit var channelName: String
  lateinit var channelDescription: String

  fun toReminder(): PomodoroReminder = PomodoroReminder(
    id, eventId, startsAtEpochMs, endsAtEpochMs, title, body, channelName, channelDescription,
  ).also { it.validate() }
}

@InvokeArg
internal class PomodoroScheduleReconcileArgs {
  var schedule: List<PomodoroReminderArgs> = listOf()
}

internal object ExactAlarmCapability {
  fun isRequired(apiLevel: Int): Boolean = apiLevel >= Build.VERSION_CODES.S

  fun isGranted(alarmManager: AlarmManager): Boolean =
    Build.VERSION.SDK_INT < Build.VERSION_CODES.S || alarmManager.canScheduleExactAlarms()
}

@InvokeArg
internal class BackgroundExecutionSettingsArgs {
  lateinit var destination: String
}

@InvokeArg
internal class AttachFocusLifecycleArgs {
  lateinit var channel: Channel
}

@InvokeArg
internal class DeviceLocalTimeArgs {
  var instants: List<Long> = listOf()
}

@TauriPlugin
class MobileNotificationsPlugin(private val activity: Activity) : Plugin(activity) {
  private val focusLifecycle = NativeFocusLifecycleBridge()

  override fun onResume() {
    super.onResume()
    focusLifecycle.observe(true)
  }

  override fun onPause() {
    focusLifecycle.observe(false)
    super.onPause()
  }

  @Command
  fun attachFocusLifecycle(invoke: Invoke) {
    focusLifecycle.attach(invoke.parseArgs(AttachFocusLifecycleArgs::class.java).channel)
    invoke.resolve()
  }

  @Command
  fun deviceLocalTimeFacts(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(DeviceLocalTimeArgs::class.java)
      val facts = DeviceLocalTimeFacts.resolve(args.instants)
      invoke.resolveObject(facts.map { fact -> JSObject().apply {
        put("epochMs", fact.epochMs)
        put("dateKey", fact.dateKey)
        put("dateString", fact.dateString)
        put("hour", fact.hour)
      } })
    } catch (error: Exception) {
      invoke.reject("Read device local time facts: ${error.message}")
    }
  }

  override fun load(webView: WebView) {
    super.load(webView)
    CalendarNotificationScheduler.captureAction(activity, activity.intent)
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    CalendarNotificationScheduler.captureAction(activity, intent)
  }

  @Command
  fun ensureCalendarChannel(invoke: Invoke) {
    val args = invoke.parseArgs(CalendarChannelArgs::class.java)
    if (args.name.isBlank()) {
      invoke.reject("Calendar notification channel name cannot be blank")
      return
    }

    try {
      val notificationManager =
        activity.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
      val audioAttributes = AudioAttributes.Builder()
        .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
        .setUsage(AudioAttributes.USAGE_NOTIFICATION_EVENT)
        .build()
      val channel = NotificationChannel(
        CALENDAR_CHANNEL_ID,
        args.name,
        NotificationManager.IMPORTANCE_HIGH,
      ).apply {
        description = args.description
        lockscreenVisibility = Notification.VISIBILITY_PRIVATE
        enableLights(true)
        enableVibration(true)
        setSound(
          RingtoneManager.getDefaultUri(RingtoneManager.TYPE_NOTIFICATION),
          audioAttributes,
        )
      }
      notificationManager.createNotificationChannel(channel)
      invoke.resolve()
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Failed to create Calendar notification channel")
    }
  }

  @Command
  fun calendarChannelStatus(invoke: Invoke) {
    val notificationManager =
      activity.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    val channel = notificationManager.getNotificationChannel(CALENDAR_CHANNEL_ID)
    invoke.resolve(JSObject().apply {
      put("exists", channel != null)
      put("enabled", channel != null && channel.importance != NotificationManager.IMPORTANCE_NONE)
      put("soundConfigured", channel?.sound != null)
    })
  }

  @Command
  fun scheduleCalendarNotifications(invoke: Invoke) {
    val args = invoke.parseArgs(CalendarNotificationBatchArgs::class.java)
    try {
      for (notification in args.notifications) {
        CalendarNotificationScheduler.schedule(
          activity,
          CalendarNotificationDelivery(
            id = notification.id,
            title = notification.title,
            body = notification.body,
            eventId = notification.eventId,
            scheduledAtEpochMs = notification.scheduledAtEpochMs,
          ),
        )
      }
      invoke.resolveObject(args.notifications.map { it.id })
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Failed to schedule Calendar notifications")
    }
  }

  @Command
  fun pendingCalendarNotifications(invoke: Invoke) {
    val pending = CalendarNotificationScheduler.pending(activity).map { notification ->
      JSObject().apply {
        put("id", notification.id)
        put("title", notification.title)
        put("body", notification.body)
        put("eventId", notification.eventId)
        put("scheduledAtEpochMs", notification.scheduledAtEpochMs)
      }
    }
    invoke.resolveObject(pending)
  }

  @Command
  fun cancelCalendarNotifications(invoke: Invoke) {
    val args = invoke.parseArgs(CalendarNotificationCancelArgs::class.java)
    CalendarNotificationScheduler.cancel(activity, args.ids)
    invoke.resolve()
  }

  @Command
  fun configureFocusNotificationCopy(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(FocusNotificationCopyArgs::class.java)
      val copy = args.copy.toCopy()
      ensurePomodoroChannels(copy)
      val scope = NativeFocusProcessScope(args.processNonce)
      scope.publish(NativeFocusAuthority::processOrUnavailable) {
        PomodoroGuardianClient(activity).configureCopy(copy, scope)
      }
      invoke.resolve()
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Failed to configure Focus notification language")
    }
  }

  @Command
  fun focusNotificationCopy(invoke: Invoke) {
    try {
      val copy = PomodoroGuardianClient(activity).copy()
      invoke.resolveObject(JSObject().apply {
        put("copy", copy?.let(PomodoroNotificationScheduler::encodeCopy))
      })
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Failed to read Focus notification language")
    }
  }

  @Command
  fun updatePomodoroNotification(invoke: Invoke) {
    val args = invoke.parseArgs(PomodoroNotificationUpdateArgs::class.java)
    try {
      val projection = args.state.toProjection()
      ensurePomodoroChannels(projection.copy)
      NativeFocusAuthority.requireCurrent(args.processNonce, args.generation, args.revision)
      PomodoroGuardianClient(activity).update(projection, NativeFocusDeliveryScope(args.processNonce, args.generation, args.revision))
      invoke.resolve()
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Failed to update Pomodoro notification")
    }
  }

  @Command
  fun completeFocusNotification(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(PomodoroNotificationUpdateArgs::class.java)
      val projection = args.state.toProjection()
      ensurePomodoroChannels(projection.copy)
      NativeFocusAuthority.requireCurrent(args.processNonce, args.generation, args.revision)
      PomodoroGuardianClient(activity).complete(projection, NativeFocusDeliveryScope(args.processNonce, args.generation, args.revision))
      invoke.resolve()
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Failed to deliver committed Focus boundary")
    }
  }

  @Command
  fun cancelPomodoroNotification(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(FocusNotificationProcessArgs::class.java)
      val scope = NativeFocusProcessScope(args.processNonce)
      scope.publish(NativeFocusAuthority::processOrUnavailable) {
        PomodoroGuardianClient(activity).cancel(scope)
      }
      invoke.resolve()
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Failed to cancel Pomodoro notification")
    }
  }

  @Command
  fun pomodoroNotificationState(invoke: Invoke) {
    try {
      val projection = PomodoroGuardianClient(activity).current()
      if (projection == null) {
        invoke.resolveObject(JSObject().apply { put("active", false) })
        return
      }
      invoke.resolveObject(projection.toResponse())
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Failed to read Pomodoro notification")
    }
  }

  @Command
  fun reconcilePomodoroSchedule(invoke: Invoke) {
    val args = invoke.parseArgs(PomodoroScheduleReconcileArgs::class.java)
    try {
      val schedule = args.schedule.map(PomodoroReminderArgs::toReminder)
      PomodoroGuardianClient(activity).reconcile(schedule)
      invoke.resolve()
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Failed to reconcile focus reminder schedule")
    }
  }

  @Command
  fun takeCalendarNotificationAction(invoke: Invoke) {
    invoke.resolveObject(JSObject().apply {
      put("eventId", CalendarNotificationScheduler.takeAction(activity))
    })
  }

  @Command
  fun exactAlarmStatus(invoke: Invoke) {
    val apiLevel = Build.VERSION.SDK_INT
    val alarmManager = activity.getSystemService(Context.ALARM_SERVICE) as AlarmManager
    invoke.resolve(JSObject().apply {
      put("apiLevel", apiLevel)
      put("required", ExactAlarmCapability.isRequired(apiLevel))
      put("granted", ExactAlarmCapability.isGranted(alarmManager))
    })
  }

  @Command
  fun backgroundExecutionStatus(invoke: Invoke) {
    val powerManager = activity.getSystemService(Context.POWER_SERVICE) as PowerManager
    val activityManager = activity.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
    val backgroundRestricted = activityManager.isBackgroundRestricted
    invoke.resolve(JSObject().apply {
      put("manufacturer", Build.MANUFACTURER.trim().take(80))
      put(
        "autostartSettingsAvailable",
        backgroundSettingsIntents(BackgroundSettingsDestination.AUTOSTART).any(::isResolvable),
      )
      put("backgroundRestricted", backgroundRestricted)
      put(
        "batteryOptimizationExempt",
        powerManager.isIgnoringBatteryOptimizations(activity.packageName),
      )
    })
  }

  @Command
  fun openBackgroundExecutionSettings(invoke: Invoke) {
    val args = invoke.parseArgs(BackgroundExecutionSettingsArgs::class.java)
    val destination = when (args.destination) {
      "autostart" -> BackgroundSettingsDestination.AUTOSTART
      "battery" -> BackgroundSettingsDestination.BATTERY
      else -> {
        invoke.reject("Unknown background execution settings destination")
        return
      }
    }
    val fallbackIntents = when (destination) {
      BackgroundSettingsDestination.AUTOSTART -> listOf(applicationDetailsIntent())
      BackgroundSettingsDestination.BATTERY -> listOf(
        Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS),
        applicationDetailsIntent(),
      )
    }
    val error = startFirstAvailable(backgroundSettingsIntents(destination) + fallbackIntents)
    if (error == null) {
      invoke.resolve()
    } else {
      invoke.reject(error.message ?: "Failed to open Android settings")
    }
  }

  @Command
  fun openExactAlarmSettings(invoke: Invoke) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) {
      invoke.resolve()
      return
    }

    try {
      activity.startActivity(
        Intent(
          Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM,
          Uri.parse("package:${activity.packageName}"),
        ),
      )
      invoke.resolve()
    } catch (error: Exception) {
      try {
        activity.startActivity(
          Intent(
            Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
            Uri.parse("package:${activity.packageName}"),
          ),
        )
        invoke.resolve()
      } catch (fallbackError: Exception) {
        invoke.reject(
          fallbackError.message ?: error.message ?: "Failed to open Android settings",
        )
      }
    }
  }

  @Command
  fun openNotificationSettings(invoke: Invoke) {
    try {
      activity.startActivity(
        Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).apply {
          putExtra(Settings.EXTRA_APP_PACKAGE, activity.packageName)
        },
      )
      invoke.resolve()
    } catch (error: Exception) {
      try {
        activity.startActivity(
          Intent(
            Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
            Uri.parse("package:${activity.packageName}"),
          ),
        )
        invoke.resolve()
      } catch (fallbackError: Exception) {
        invoke.reject(
          fallbackError.message ?: error.message ?: "Failed to open Android settings",
        )
      }
    }
  }

  private fun ensurePomodoroChannels(copy: PomodoroNotificationCopy) {
    val manager = activity.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    manager.createNotificationChannel(
      NotificationChannel(
        POMODORO_CHANNEL_ID,
        copy.channelName,
        NotificationManager.IMPORTANCE_LOW,
      ).apply {
        description = copy.channelDescription
        lockscreenVisibility = Notification.VISIBILITY_PRIVATE
        setSound(null, null)
        enableVibration(false)
        enableLights(false)
        setShowBadge(false)
      },
    )
    manager.createNotificationChannel(
      NotificationChannel(
        POMODORO_ALERTS_CHANNEL_ID,
        copy.alertsChannelName,
        NotificationManager.IMPORTANCE_HIGH,
      ).apply {
        description = copy.alertsChannelDescription
        lockscreenVisibility = Notification.VISIBILITY_PRIVATE
        enableLights(true)
        enableVibration(true)
        setSound(
          RingtoneManager.getDefaultUri(RingtoneManager.TYPE_NOTIFICATION),
          AudioAttributes.Builder()
            .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
            .setUsage(AudioAttributes.USAGE_NOTIFICATION_EVENT)
            .build(),
        )
      },
    )
  }

  private fun backgroundSettingsIntents(
    destination: BackgroundSettingsDestination,
  ): List<Intent> = BackgroundExecutionSettingsRegistry.candidates(
    destination = destination,
    manufacturer = Build.MANUFACTURER,
    brand = Build.BRAND,
  ).map(::backgroundSettingsIntent)

  private fun backgroundSettingsIntent(spec: BackgroundSettingsIntentSpec): Intent = when (spec) {
    is BackgroundSettingsIntentSpec.Component -> Intent().apply {
      component = ComponentName(spec.packageName, spec.className)
      spec.dataUri?.let { data = Uri.parse(it) }
      if (spec.includeApplicationExtras) {
        putExtra("package_name", activity.packageName)
        putExtra(
          "package_label",
          activity.applicationInfo.loadLabel(activity.packageManager).toString(),
        )
      }
    }
    is BackgroundSettingsIntentSpec.Action -> Intent(spec.action).apply {
      spec.packageName?.let(::setPackage)
      spec.integerExtras.forEach { (key, value) -> putExtra(key, value) }
    }
  }

  private fun startFirstAvailable(intents: List<Intent>): Exception? {
    var lastError: Exception? = null
    for (intent in intents) {
      if (!isResolvable(intent)) continue
      try {
        activity.startActivity(intent)
        return null
      } catch (error: Exception) {
        lastError = error
      }
    }
    return lastError ?: IllegalStateException("No compatible Android settings screen is available")
  }

  private fun isResolvable(intent: Intent): Boolean =
    intent.resolveActivity(activity.packageManager) != null

  private fun applicationDetailsIntent(): Intent = Intent(
    Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
    Uri.parse("package:${activity.packageName}"),
  )
}

private fun PomodoroNotificationCopyArgs.toCopy(): PomodoroNotificationCopy =
  PomodoroNotificationCopy(
    channelName, channelDescription, alertsChannelName, alertsChannelDescription,
    focusTitle, shortBreakTitle, longBreakTitle, pausedText,
    focusCompleteTitle, breakCompleteTitle, sessionCompleteText,
  )

private fun PomodoroNotificationStateArgs.toProjection(): PomodoroNotificationProjection =
  PomodoroNotificationProjection(
    runId = runId,
    eventId = eventId,
    eventTitle = eventTitle?.trim()?.takeIf(String::isNotEmpty),
    eventDate = eventDate,
    eventEndsAtEpochMs = eventEndsAtEpochMs,
    generatedAtEpochMs = generatedAtEpochMs,
    isRunning = isRunning,
    remainingSeconds = remainingSeconds,
    totalSeconds = totalSeconds,
    configJson = configJson,
    phases = phases.map { phase ->
      PomodoroNotificationPhase(
        id = phase.id,
        phase = phase.phase,
        rhythmPosition = phase.rhythmPosition,
        startsAtEpochMs = phase.startsAtEpochMs,
        endsAtEpochMs = phase.endsAtEpochMs,
      )
    },
    copy = PomodoroNotificationCopy(
      channelName = copy.channelName,
      channelDescription = copy.channelDescription,
      alertsChannelName = copy.alertsChannelName,
      alertsChannelDescription = copy.alertsChannelDescription,
      focusTitle = copy.focusTitle,
      shortBreakTitle = copy.shortBreakTitle,
      longBreakTitle = copy.longBreakTitle,
      pausedText = copy.pausedText,
      focusCompleteTitle = copy.focusCompleteTitle,
      breakCompleteTitle = copy.breakCompleteTitle,
      sessionCompleteText = copy.sessionCompleteText,
    ),
  )

private fun PomodoroNotificationProjection.toResponse(): JSObject = JSObject().apply {
  put("active", true)
  put("runId", runId)
  put("eventId", eventId)
  put("eventTitle", eventTitle)
  put("eventDate", eventDate)
  put("eventEndsAtEpochMs", eventEndsAtEpochMs)
  put("generatedAtEpochMs", generatedAtEpochMs)
  put("isRunning", isRunning)
  put("remainingSeconds", remainingSeconds)
  put("totalSeconds", totalSeconds)
  put("configJson", configJson)
  put("phases", JSArray().apply {
    phases.forEach { phase ->
      put(JSObject().apply {
        put("id", phase.id)
        put("phase", phase.phase)
        put("rhythmPosition", phase.rhythmPosition)
        put("startsAtEpochMs", phase.startsAtEpochMs)
        put("endsAtEpochMs", phase.endsAtEpochMs)
      })
    }
  })
}
