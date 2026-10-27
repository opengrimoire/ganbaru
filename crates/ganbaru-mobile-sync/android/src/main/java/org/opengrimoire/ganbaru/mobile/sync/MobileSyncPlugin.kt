package org.opengrimoire.ganbaru.mobile.sync

import android.app.Activity
import android.app.ActivityManager
import android.app.usage.UsageStatsManager
import android.content.Context
import android.net.ConnectivityManager
import android.net.LinkProperties
import android.net.Network
import android.os.Build
import android.util.Log
import android.webkit.WebView
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.OutOfQuotaPolicy
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.workDataOf
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.util.concurrent.TimeUnit

@InvokeArg
internal class ConfigureArgs {
  var linked: Boolean = false
  var pending: Boolean = false
  lateinit var configDir: String
}

/**
 * Schedules background sync through WorkManager: a periodic pass while linked, and a one-time pass
 * when the default network changes or the app leaves the foreground with changes waiting. Work is
 * never run as a foreground service.
 */
@TauriPlugin
class MobileSyncPlugin(private val activity: Activity) : Plugin(activity) {
  private val context: Context = activity.applicationContext
  private val preferences = SyncPreferences(context)
  private val observations = NetworkObservations()

  @Volatile
  private var linked = preferences.read().linked

  @Volatile
  private var pending = false

  private var networkCallback: ConnectivityManager.NetworkCallback? = null

  init {
    // Rust reads sealed key files soon after startup, so the Keystore bridge registers first.
    SyncNative.ensureLoaded()
  }

  override fun load(webView: WebView) {
    val connectivity = context.getSystemService(ConnectivityManager::class.java) ?: return
    val callback = object : ConnectivityManager.NetworkCallback() {
      override fun onLinkPropertiesChanged(network: Network, linkProperties: LinkProperties) {
        val addresses = linkProperties.linkAddresses.map { it.toString() }.toSet()
        if (observations.observe(network.networkHandle, addresses) && linked) {
          enqueueOneTime(expedited = false)
        }
      }

      override fun onLost(network: Network) {
        observations.lost()
      }
    }
    try {
      connectivity.registerDefaultNetworkCallback(callback)
      networkCallback = callback
    } catch (error: RuntimeException) {
      Log.w(TAG, "Network changes will not trigger background sync", error)
    }
  }

  override fun onStop() {
    if (linked && pending) enqueueOneTime(expedited = true)
  }

  override fun onDestroy() {
    val callback = networkCallback ?: return
    networkCallback = null
    try {
      context.getSystemService(ConnectivityManager::class.java)?.unregisterNetworkCallback(callback)
    } catch (error: IllegalArgumentException) {
      Log.w(TAG, "The network callback was not registered", error)
    }
  }

  @Command
  fun configure(invoke: Invoke) {
    val args = invoke.parseArgs(ConfigureArgs::class.java)
    if (args.configDir.isEmpty()) {
      invoke.reject("The app config directory is required")
      return
    }
    preferences.write(SyncSettings(args.linked, args.configDir))
    linked = args.linked
    pending = args.pending
    val work = WorkManager.getInstance(context)
    if (args.linked) {
      val request = PeriodicWorkRequestBuilder<SyncWorker>(
        SyncScheduling.PERIODIC_INTERVAL_MINUTES,
        TimeUnit.MINUTES,
      ).setConstraints(connected()).build()
      work.enqueueUniquePeriodicWork(SyncScheduling.PERIODIC_WORK, ExistingPeriodicWorkPolicy.UPDATE, request)
    } else {
      work.cancelUniqueWork(SyncScheduling.PERIODIC_WORK)
      work.cancelUniqueWork(SyncScheduling.ONE_TIME_WORK)
    }
    invoke.resolve()
  }

  @Command
  fun connectivityStatus(invoke: Invoke) {
    val activities = context.getSystemService(ActivityManager::class.java)
    val usage = context.getSystemService(UsageStatsManager::class.java)
    val standbyRestricted = Build.VERSION.SDK_INT >= Build.VERSION_CODES.R &&
      usage?.appStandbyBucket == UsageStatsManager.STANDBY_BUCKET_RESTRICTED
    invoke.resolve(JSObject().apply {
      put("backgroundRestricted", activities?.isBackgroundRestricted == true)
      put("standbyRestricted", standbyRestricted)
    })
  }

  private fun connected(): Constraints = Constraints.Builder()
    .setRequiredNetworkType(NetworkType.CONNECTED)
    .build()

  private fun enqueueOneTime(expedited: Boolean) {
    val builder = OneTimeWorkRequestBuilder<SyncWorker>()
      .setConstraints(connected())
      .setBackoffCriteria(BackoffPolicy.LINEAR, SyncScheduling.RETRY_BACKOFF_SECONDS, TimeUnit.SECONDS)
      .setInputData(workDataOf(SyncWorker.KEY_ONE_TIME to true))
    if (expedited && SyncScheduling.oneTimeRequest(Build.VERSION.SDK_INT) == OneTimeRequest.EXPEDITED) {
      builder.setExpedited(OutOfQuotaPolicy.RUN_AS_NON_EXPEDITED_WORK_REQUEST)
    }
    WorkManager.getInstance(context)
      .enqueueUniqueWork(SyncScheduling.ONE_TIME_WORK, ExistingWorkPolicy.KEEP, builder.build())
  }

  private companion object {
    const val TAG = "GanbaruSync"
  }
}
