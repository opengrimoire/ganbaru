package org.opengrimoire.ganbaru.mobile.sync

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import androidx.work.CoroutineWorker
import androidx.work.WorkerParameters
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Runs headless sync passes for WorkManager. A run does nothing unless this device is linked and on
 * Wi-Fi, Ethernet, or a VPN; an unreachable coordinator is browsed for once per run.
 */
class SyncWorker(context: Context, parameters: WorkerParameters) : CoroutineWorker(context, parameters) {
  override suspend fun doWork(): Result {
    val settings = SyncPreferences(applicationContext).read()
    val configDir = settings.configDir
    if (configDir == null || !SyncScheduling.shouldRun(settings.linked, onLocalNetwork())) {
      return Result.success()
    }
    var result = withContext(Dispatchers.IO) { SyncNative.run(configDir, null) }
    var followUp = SyncScheduling.followUp(result, browsed = false)
    if (followUp == PassFollowUp.BROWSE) {
      val records = CoordinatorBrowser(applicationContext).browse(result.discoveryNames)
      if (records != null) {
        result = withContext(Dispatchers.IO) { SyncNative.run(configDir, records) }
      }
      followUp = SyncScheduling.followUp(result, browsed = true)
    }
    val oneTime = inputData.getBoolean(KEY_ONE_TIME, false)
    return when (SyncScheduling.runEnd(followUp, oneTime, runAttemptCount)) {
      RunEnd.RETRY -> Result.retry()
      RunEnd.SUCCESS -> Result.success()
    }
  }

  private fun onLocalNetwork(): Boolean {
    val connectivity = applicationContext.getSystemService(ConnectivityManager::class.java) ?: return false
    val capabilities = connectivity.getNetworkCapabilities(connectivity.activeNetwork) ?: return false
    return LOCAL_TRANSPORTS.any { capabilities.hasTransport(it) }
  }

  internal companion object {
    /** Input data key marking a one-time run, which may retry. */
    const val KEY_ONE_TIME = "oneTime"
    private val LOCAL_TRANSPORTS = listOf(
      NetworkCapabilities.TRANSPORT_WIFI,
      NetworkCapabilities.TRANSPORT_ETHERNET,
      NetworkCapabilities.TRANSPORT_VPN,
    )
  }
}
