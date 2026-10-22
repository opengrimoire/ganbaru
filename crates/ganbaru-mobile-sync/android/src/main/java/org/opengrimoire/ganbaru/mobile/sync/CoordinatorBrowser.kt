package org.opengrimoire.ganbaru.mobile.sync

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.os.Build
import android.util.Log
import kotlin.coroutines.resume
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withTimeoutOrNull
import org.json.JSONArray
import org.json.JSONObject

/**
 * Browses the local network for the pinned coordinator under the instance names Rust derived, and
 * resolves the matches into the records JSON the next pass reads. Only names Rust expects are
 * resolved, and Rust validates every record again.
 */
internal class CoordinatorBrowser(context: Context) {
  private val nsd = context.applicationContext.getSystemService(NsdManager::class.java)

  /** Records JSON for the services found under `names`, or `null` when none resolved. */
  suspend fun browse(names: List<String>): String? {
    val manager = nsd ?: return null
    val wanted = names.toSet()
    if (wanted.isEmpty()) return null
    val services = discover(manager, wanted)
    val records = JSONArray()
    for (service in services.take(MAX_RECORDS)) {
      val resolved = withTimeoutOrNull(SyncScheduling.DISCOVERY_BROWSE_MILLIS) { resolve(manager, service) }
      if (resolved != null) records.put(record(resolved))
    }
    return if (records.length() == 0) null else records.toString()
  }

  /** Services found under a wanted name within the browse window; it ends early on the first one. */
  private suspend fun discover(manager: NsdManager, wanted: Set<String>): List<NsdServiceInfo> {
    val found = mutableListOf<NsdServiceInfo>()
    val first = CompletableDeferred<Unit>()
    val listener = object : NsdManager.DiscoveryListener {
      override fun onDiscoveryStarted(serviceType: String) {}

      override fun onDiscoveryStopped(serviceType: String) {}

      override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
        Log.w(TAG, "Coordinator discovery did not start: $errorCode")
        first.complete(Unit)
      }

      override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {
        Log.w(TAG, "Coordinator discovery did not stop: $errorCode")
      }

      override fun onServiceFound(serviceInfo: NsdServiceInfo) {
        if (serviceInfo.serviceName !in wanted) return
        synchronized(found) {
          if (found.none { it.serviceName == serviceInfo.serviceName }) found.add(serviceInfo)
        }
        first.complete(Unit)
      }

      override fun onServiceLost(serviceInfo: NsdServiceInfo) {}
    }
    manager.discoverServices(SyncScheduling.SERVICE_TYPE, NsdManager.PROTOCOL_DNS_SD, listener)
    try {
      withTimeoutOrNull(SyncScheduling.DISCOVERY_BROWSE_MILLIS) { first.await() }
    } finally {
      try {
        manager.stopServiceDiscovery(listener)
      } catch (error: IllegalArgumentException) {
        Log.w(TAG, "Coordinator discovery was not running", error)
      }
    }
    return synchronized(found) { found.toList() }
  }

  /**
   * Resolves one service. The listener API is deprecated from Android 14 but remains available,
   * and it is the only resolver on the earlier releases this app supports.
   */
  @Suppress("DEPRECATION")
  private suspend fun resolve(manager: NsdManager, service: NsdServiceInfo): NsdServiceInfo? =
    suspendCancellableCoroutine { continuation ->
      manager.resolveService(
        service,
        object : NsdManager.ResolveListener {
          override fun onResolveFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {
            Log.w(TAG, "Coordinator service did not resolve: $errorCode")
            if (continuation.isActive) continuation.resume(null)
          }

          override fun onServiceResolved(serviceInfo: NsdServiceInfo) {
            if (continuation.isActive) continuation.resume(serviceInfo)
          }
        },
      )
    }

  @Suppress("DEPRECATION")
  private fun record(service: NsdServiceInfo): JSONObject {
    val hosts = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
      service.hostAddresses
    } else {
      listOfNotNull(service.host)
    }
    val addresses = JSONArray()
    hosts.mapNotNull { it.hostAddress }.distinct().take(MAX_ADDRESSES).forEach { addresses.put(it) }
    return JSONObject().apply {
      put("instanceName", service.serviceName)
      put("port", service.port)
      put("addresses", addresses)
      service.attributes[TXT_VERSION_KEY]?.let { put("version", String(it, Charsets.UTF_8)) }
    }
  }

  private companion object {
    const val TAG = "GanbaruSync"
    /** Rust accepts at most this many records. */
    const val MAX_RECORDS = 16
    /** Rust drops a record with more addresses than this. */
    const val MAX_ADDRESSES = 8
    const val TXT_VERSION_KEY = "v"
  }
}
