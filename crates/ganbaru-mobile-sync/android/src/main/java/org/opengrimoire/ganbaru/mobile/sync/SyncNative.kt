package org.opengrimoire.ganbaru.mobile.sync

import android.util.Log
import org.json.JSONException
import org.json.JSONObject

/**
 * The Rust background pass entry, loaded on demand when WorkManager starts the process. Loading
 * also registers [SyncKeystore] with Rust, so the app and a background pass seal key files with the
 * same Android Keystore keys.
 */
internal object SyncNative {
  /** The Rust library name, which the Android project check matches against the Cargo library name. */
  const val NATIVE_LIBRARY = "ganbaru_lib"
  private const val TAG = "GanbaruSync"

  private val loaded: Boolean by lazy {
    try {
      System.loadLibrary(NATIVE_LIBRARY)
    } catch (error: UnsatisfiedLinkError) {
      Log.e(TAG, "Native background sync is not available", error)
      return@lazy false
    } catch (error: SecurityException) {
      Log.e(TAG, "Native background sync could not be loaded", error)
      return@lazy false
    }
    try {
      registerKeystore(SyncKeystore)
    } catch (error: LinkageError) {
      Log.e(TAG, "Native key protection is not linked", error)
    }
    true
  }

  /** Hands Rust the object that seals device-local key files with the Android Keystore. */
  @JvmStatic
  external fun registerKeystore(keystore: SyncKeystore)

  /**
   * Loads the library and registers the key store bridge once per process. The plugin calls it
   * when the app starts, before Rust needs a sealed key; returns whether the library loaded.
   */
  fun ensureLoaded(): Boolean = loaded

  /**
   * Runs one headless pass for the app config directory with an optional input, either a JSON
   * array of discovered coordinator records or a relay network object from
   * [RelayScheduling.passInput], and returns the JSON result, or `null` when Rust could not
   * create it.
   */
  @JvmStatic
  external fun runPass(configDir: String, records: String?): String?

  /** Runs one pass and reads its result; an unavailable library or unreadable result is a failure. */
  fun run(configDir: String, records: String?): PassResult {
    if (!loaded) return PassResult.FAILED
    val json = try {
      runPass(configDir, records)
    } catch (error: LinkageError) {
      Log.e(TAG, "Native background sync is not linked", error)
      null
    } ?: return PassResult.FAILED
    return parse(json)
  }

  private fun parse(json: String): PassResult = try {
    val value = JSONObject(json)
    val names = value.optJSONArray("discoveryNames")
    PassResult(
      PassOutcome.fromWire(value.optString("outcome")),
      if (names == null) emptyList() else (0 until names.length()).map { names.getString(it) },
    )
  } catch (error: JSONException) {
    Log.e(TAG, "Native background sync returned an unreadable result", error)
    PassResult.FAILED
  }
}
