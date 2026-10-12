package org.opengrimoire.ganbaruai.mobile.notifications

import android.app.AlarmManager
import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import org.json.JSONObject

internal const val CALENDAR_CHANNEL_ID = "calendar-events"
private const val CALENDAR_NOTIFICATION_STORE = "GANBARU_CALENDAR_NOTIFICATION_STORE"
private const val CALENDAR_NOTIFICATION_ACTION_STORE = "GANBARU_CALENDAR_NOTIFICATION_ACTION_STORE"
private const val PENDING_ACTION_EVENT_ID = "eventId"
private const val EXTRA_ID = "ganbaruCalendarNotificationId"
private const val EXTRA_TITLE = "ganbaruCalendarNotificationTitle"
private const val EXTRA_BODY = "ganbaruCalendarNotificationBody"
private const val EXTRA_EVENT_ID = "ganbaruCalendarNotificationEventId"

internal data class CalendarNotificationDelivery(
  val id: Int,
  val title: String,
  val body: String,
  val eventId: String,
  val scheduledAtEpochMs: Long,
)

internal object CalendarNotificationScheduler {
  fun schedule(context: Context, delivery: CalendarNotificationDelivery) {
    require(delivery.id > 0) { "Calendar notification ID must be positive" }
    require(delivery.title.isNotBlank() && delivery.title.length <= 160) {
      "Calendar notification title must contain 1 to 160 characters"
    }
    require(delivery.body.length <= 1_000) {
      "Calendar notification body must not exceed 1000 characters"
    }
    require(delivery.eventId.isNotBlank() && delivery.eventId.length <= 256) {
      "Calendar event ID must contain 1 to 256 characters"
    }
    require(delivery.scheduledAtEpochMs > System.currentTimeMillis()) {
      "Calendar notification must be scheduled in the future"
    }

    scheduleAlarm(context, delivery)
    save(context, delivery)
  }

  fun pending(context: Context): List<CalendarNotificationDelivery> =
    store(context).all.values.mapNotNull { encoded ->
      val value = encoded as? String ?: return@mapNotNull null
      decode(value)
    }

  fun cancel(context: Context, ids: List<Int>) {
    val editor = store(context).edit()
    for (id in ids) {
      if (id <= 0) continue
      alarmManager(context).cancel(pendingIntent(context, id, null))
      editor.remove(id.toString())
      (context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager).cancel(id)
    }
    editor.apply()
  }

  fun restore(context: Context) {
    val now = System.currentTimeMillis()
    val editor = store(context).edit()
    for (delivery in pending(context)) {
      if (delivery.scheduledAtEpochMs <= now) {
        editor.remove(delivery.id.toString())
      } else {
        scheduleAlarm(context, delivery)
      }
    }
    editor.apply()
  }

  fun deliver(context: Context, intent: Intent) {
    val delivery = deliveryFromIntent(intent) ?: return
    post(context, delivery.id, delivery.title, delivery.body, delivery.eventId)
    store(context).edit().remove(delivery.id.toString()).apply()
  }

  private fun post(
    context: Context,
    id: Int,
    title: String,
    body: String,
    eventId: String?,
  ) {
    val launchIntent = context.packageManager.getLaunchIntentForPackage(context.packageName)
      ?.apply {
        flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
        if (eventId != null) putExtra(EXTRA_EVENT_ID, eventId)
      }
    val contentIntent = launchIntent?.let {
      PendingIntent.getActivity(
        context,
        id,
        it,
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
      )
    }
    val icon = context.resources.getIdentifier(
      "ic_notification_calendar",
      "drawable",
      context.packageName,
    ).takeIf { it != 0 } ?: context.applicationInfo.icon
    val notification = Notification.Builder(context, CALENDAR_CHANNEL_ID)
      .setSmallIcon(icon)
      .setContentTitle(title)
      .setContentText(body)
      .setCategory(Notification.CATEGORY_EVENT)
      .setVisibility(Notification.VISIBILITY_PRIVATE)
      .setAutoCancel(true)
      .setContentIntent(contentIntent)
      .build()
    (context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager)
      .notify(id, notification)
  }

  fun captureAction(context: Context, intent: Intent?) {
    val eventId = intent?.getStringExtra(EXTRA_EVENT_ID)?.takeIf {
      it.isNotBlank() && it.length <= 256
    } ?: return
    context.getSharedPreferences(CALENDAR_NOTIFICATION_ACTION_STORE, Context.MODE_PRIVATE)
      .edit()
      .putString(PENDING_ACTION_EVENT_ID, eventId)
      .apply()
    intent.removeExtra(EXTRA_EVENT_ID)
  }

  fun takeAction(context: Context): String? {
    val preferences = context.getSharedPreferences(
      CALENDAR_NOTIFICATION_ACTION_STORE,
      Context.MODE_PRIVATE,
    )
    val eventId = preferences.getString(PENDING_ACTION_EVENT_ID, null)
    preferences.edit().remove(PENDING_ACTION_EVENT_ID).apply()
    return eventId?.takeIf { it.isNotBlank() && it.length <= 256 }
  }

  private fun scheduleAlarm(context: Context, delivery: CalendarNotificationDelivery) {
    val pendingIntent = pendingIntent(context, delivery.id, delivery)
    val manager = alarmManager(context)
    manager.scheduleRtcWakeupAllowingIdle(delivery.scheduledAtEpochMs, pendingIntent)
  }

  private fun pendingIntent(
    context: Context,
    id: Int,
    delivery: CalendarNotificationDelivery?,
  ): PendingIntent {
    val intent = Intent(context, CalendarNotificationReceiver::class.java).apply {
      if (delivery != null) {
        putExtra(EXTRA_ID, delivery.id)
        putExtra(EXTRA_TITLE, delivery.title)
        putExtra(EXTRA_BODY, delivery.body)
        putExtra(EXTRA_EVENT_ID, delivery.eventId)
      }
    }
    return PendingIntent.getBroadcast(
      context,
      id,
      intent,
      PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )
  }

  private fun deliveryFromIntent(intent: Intent): CalendarNotificationDelivery? {
    val id = intent.getIntExtra(EXTRA_ID, 0)
    val title = intent.getStringExtra(EXTRA_TITLE) ?: return null
    val body = intent.getStringExtra(EXTRA_BODY) ?: return null
    val eventId = intent.getStringExtra(EXTRA_EVENT_ID) ?: return null
    if (id <= 0 || title.isBlank() || eventId.isBlank()) return null
    return CalendarNotificationDelivery(id, title, body, eventId, 0)
  }

  private fun save(context: Context, delivery: CalendarNotificationDelivery) {
    store(context).edit().putString(delivery.id.toString(), encode(delivery)).apply()
  }

  private fun encode(delivery: CalendarNotificationDelivery): String = JSONObject()
    .put("id", delivery.id)
    .put("title", delivery.title)
    .put("body", delivery.body)
    .put("eventId", delivery.eventId)
    .put("scheduledAtEpochMs", delivery.scheduledAtEpochMs)
    .toString()

  private fun decode(encoded: String): CalendarNotificationDelivery? = try {
    val value = JSONObject(encoded)
    CalendarNotificationDelivery(
      id = value.getInt("id"),
      title = value.getString("title"),
      body = value.getString("body"),
      eventId = value.getString("eventId"),
      scheduledAtEpochMs = value.getLong("scheduledAtEpochMs"),
    )
  } catch (_: Exception) {
    null
  }

  private fun store(context: Context) = context.getSharedPreferences(
    CALENDAR_NOTIFICATION_STORE,
    Context.MODE_PRIVATE,
  )

  private fun alarmManager(context: Context) =
    context.getSystemService(Context.ALARM_SERVICE) as AlarmManager
}

class CalendarNotificationReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent) {
    CalendarNotificationScheduler.deliver(context, intent)
  }
}

class CalendarNotificationRestoreReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent) {
    if (!isNotificationRestoreAction(intent.action)) return
    CalendarNotificationScheduler.restore(context)
  }
}

internal fun isNotificationRestoreAction(action: String?): Boolean = action in setOf(
  Intent.ACTION_BOOT_COMPLETED,
  Intent.ACTION_MY_PACKAGE_REPLACED,
  Intent.ACTION_TIME_CHANGED,
  Intent.ACTION_TIMEZONE_CHANGED,
)
