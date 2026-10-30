package org.opengrimoire.ganbaru.mobile.sync

import android.content.Context

/**
 * The scheduling state a worker reads when the app is not running: whether this device syncs with
 * a coordinator, whether the vault's sync space uses a relay, and the app config directory.
 */
internal data class SyncSettings(val linked: Boolean, val relay: Boolean, val configDir: String?)

/** App-private storage for the scheduling state Rust sends. */
internal class SyncPreferences(context: Context) {
  private val preferences = context.applicationContext.getSharedPreferences(FILE, Context.MODE_PRIVATE)

  fun read(): SyncSettings = SyncSettings(
    linked = preferences.getBoolean(KEY_LINKED, false),
    relay = preferences.getBoolean(KEY_RELAY, false),
    configDir = preferences.getString(KEY_CONFIG_DIR, null),
  )

  fun write(settings: SyncSettings) {
    preferences.edit()
      .putBoolean(KEY_LINKED, settings.linked)
      .putBoolean(KEY_RELAY, settings.relay)
      .putString(KEY_CONFIG_DIR, settings.configDir)
      .apply()
  }

  private companion object {
    const val FILE = "ganbaru_background_sync"
    const val KEY_LINKED = "linked"
    const val KEY_RELAY = "relay"
    const val KEY_CONFIG_DIR = "configDir"
  }
}
