package org.opengrimoire.ganbaruai.mobile.notifications

import android.app.AlarmManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import org.json.JSONObject

private const val REMINDER_STORE = "GANBARU_FOCUS_REMINDER_STORE"
private const val DELIVERED_KEY = "deliveredReminderIds"
private const val EXTRA_REMINDER_ID = "ganbaruFocusReminderId"
private const val REQUEST_CODE_START = 1_600_000_000
private const val REQUEST_CODE_SPAN = 100_000_000
private const val MAX_REMINDERS = 128

/** A scheduled commitment has no run ID, active phase, or execution authority. */
internal data class PomodoroReminder(
  val id: String,
  val eventId: String,
  val startsAtEpochMs: Long,
  val endsAtEpochMs: Long,
  val title: String,
  val body: String,
  val channelName: String,
  val channelDescription: String,
) {
  fun validate() {
    require(id.isNotBlank() && id.length <= 128) { "Focus reminder ID is invalid" }
    require(eventId.isNotBlank() && eventId.length <= 256) { "Focus reminder event is invalid" }
    require(startsAtEpochMs > 0 && startsAtEpochMs < endsAtEpochMs) {
      "Focus reminder window is invalid"
    }
    require(title.isNotBlank() && title.length <= 160) { "Focus reminder title is invalid" }
    require(body.isNotBlank() && body.length <= 1_000) { "Focus reminder text is invalid" }
    require(channelName.isNotBlank() && channelName.length <= 160) {
      "Focus reminder channel is invalid"
    }
    require(channelDescription.length <= 1_000) { "Focus reminder channel description is invalid" }
  }

  fun isDue(now: Long, delivered: Set<String>): Boolean =
    id !in delivered && startsAtEpochMs <= now && now < endsAtEpochMs

  fun encode(): String = JSONObject()
    .put("id", id)
    .put("eventId", eventId)
    .put("startsAtEpochMs", startsAtEpochMs)
    .put("endsAtEpochMs", endsAtEpochMs)
    .put("title", title)
    .put("body", body)
    .put("channelName", channelName)
    .put("channelDescription", channelDescription)
    .toString()

  companion object {
    fun decode(encoded: String): PomodoroReminder {
      val value = JSONObject(encoded)
      return PomodoroReminder(
        id = value.getString("id"),
        eventId = value.getString("eventId"),
        startsAtEpochMs = value.getLong("startsAtEpochMs"),
        endsAtEpochMs = value.getLong("endsAtEpochMs"),
        title = value.getString("title"),
        body = value.getString("body"),
        channelName = value.getString("channelName"),
        channelDescription = value.getString("channelDescription"),
      ).also { it.validate() }
    }
  }
}

/** Persists and delivers reminders without touching timer state or native focus effects. */
internal object PomodoroReminderScheduler {
  fun reconcile(context: Context, schedule: List<PomodoroReminder>) {
    require(schedule.size <= MAX_REMINDERS) { "Too many focus reminders" }
    val identifiers = mutableSetOf<String>()
    val requestCodes = mutableSetOf<Int>()
    schedule.forEach {
      it.validate()
      require(identifiers.add(it.id)) { "Focus reminder IDs must be unique" }
      require(requestCodes.add(requestCode(it.id))) { "Focus reminder alarm identifier collision" }
    }
    val previous = storedReminders(context)
    val delivered = delivered(context).intersect(identifiers)
    val editor = store(context).edit().clear().putStringSet(DELIVERED_KEY, delivered)
    schedule.forEach { editor.putString(it.id, it.encode()) }
    check(editor.commit()) { "Focus reminders could not be persisted" }
    previous.forEach {
      cancelAlarm(context, it.id)
      if (it.id !in identifiers) cancelNotification(context, it.id)
    }
    restore(context)
  }

  fun restore(context: Context) {
    val now = System.currentTimeMillis()
    storedReminders(context).forEach { reminder ->
      when {
        now >= reminder.endsAtEpochMs -> {
          cancelAlarm(context, reminder.id)
          cancelNotification(context, reminder.id)
          check(store(context).edit().remove(reminder.id).commit()) {
            "Expired focus reminder could not be removed"
          }
        }
        reminder.isDue(now, delivered(context)) -> deliver(context, reminder.id)
        reminder.id !in delivered(context) -> alarmManager(context)
          .scheduleRtcWakeupAllowingIdle(reminder.startsAtEpochMs, pendingIntent(context, reminder.id))
      }
    }
  }

  fun deliver(context: Context, id: String?) {
    val reminder = storedReminders(context).firstOrNull { it.id == id } ?: return
    val delivered = delivered(context)
    if (!reminder.isDue(System.currentTimeMillis(), delivered)) return
    val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    if (!manager.areNotificationsEnabled()) return
    manager.createNotificationChannel(NotificationChannel(
      POMODORO_ALERTS_CHANNEL_ID, reminder.channelName, NotificationManager.IMPORTANCE_HIGH,
    ).apply {
      description = reminder.channelDescription
      lockscreenVisibility = Notification.VISIBILITY_PRIVATE
    })
    if (manager.getNotificationChannel(POMODORO_ALERTS_CHANNEL_ID)?.importance == NotificationManager.IMPORTANCE_NONE) return
    val launchIntent = requireNotNull(context.packageManager.getLaunchIntentForPackage(context.packageName))
    val contentIntent = PendingIntent.getActivity(
      context, requestCode(reminder.id), launchIntent,
      PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )
    val notification = Notification.Builder(context, POMODORO_ALERTS_CHANNEL_ID)
      .setSmallIcon(R.drawable.ic_notification_focus)
      .setContentTitle(reminder.title)
      .setContentText(reminder.body)
      .setCategory(Notification.CATEGORY_REMINDER)
      .setVisibility(Notification.VISIBILITY_PRIVATE)
      .setContentIntent(contentIntent)
      .setAutoCancel(true)
      .build()
    // A crash after this receipt may lose an alert, but cannot repeat it or create execution.
    check(store(context).edit().putStringSet(DELIVERED_KEY, delivered + reminder.id).commit()) {
      "Focus reminder receipt could not be persisted"
    }
    manager.notify(requestCode(reminder.id), notification)
  }

  private fun storedReminders(context: Context): List<PomodoroReminder> = store(context).all
    .filterKeys { it != DELIVERED_KEY }
    .map { (_, value) -> PomodoroReminder.decode(value as? String ?: error("Invalid focus reminder record")) }

  private fun delivered(context: Context): Set<String> =
    store(context).getStringSet(DELIVERED_KEY, emptySet())?.toSet() ?: emptySet()

  private fun cancelAlarm(context: Context, id: String) =
    alarmManager(context).cancel(pendingIntent(context, id))

  private fun cancelNotification(context: Context, id: String) =
    (context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager).cancel(requestCode(id))

  private fun requestCode(id: String): Int =
    REQUEST_CODE_START + (id.hashCode() and Int.MAX_VALUE) % REQUEST_CODE_SPAN

  private fun pendingIntent(context: Context, id: String): PendingIntent = PendingIntent.getBroadcast(
    context, requestCode(id), Intent(context, PomodoroReminderReceiver::class.java).apply {
      putExtra(EXTRA_REMINDER_ID, id)
    }, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
  )

  private fun store(context: Context) = context.getSharedPreferences(REMINDER_STORE, Context.MODE_PRIVATE)

  private fun alarmManager(context: Context) = context.getSystemService(Context.ALARM_SERVICE) as AlarmManager
}

class PomodoroReminderReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent) {
    synchronized(POMODORO_GUARDIAN_LOCK) {
      try {
        PomodoroReminderScheduler.deliver(context, intent.getStringExtra(EXTRA_REMINDER_ID))
      } catch (error: Exception) {
        android.util.Log.e("GanbaruFocus", "Focus reminder delivery failed", error)
      }
    }
  }
}
