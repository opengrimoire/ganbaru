package org.opengrimoire.ganbaru.mobile.sync

import android.content.Context

/** The scheduling state a worker reads when the app is not running. */
internal data class SyncSettings(val linked: Boolean, val configDir: String?)

/** App-private storage for the scheduling state Rust sends. */
internal class SyncPreferences(context: Context) {
  private val preferences = context.applicationContext.getSharedPreferences(FILE, Context.MODE_PRIVATE)

  fun read(): SyncSettings = SyncSettings(
    linked = preferences.getBoolean(KEY_LINKED, false),
    configDir = preferences.getString(KEY_CONFIG_DIR, null),
  )

  fun write(settings: SyncSettings) {
    preferences.edit()
      .putBoolean(KEY_LINKED, settings.linked)
      .putString(KEY_CONFIG_DIR, settings.configDir)
      .apply()
  }

  private companion object {
    const val FILE = "ganbaru_background_sync"
    const val KEY_LINKED = "linked"
    const val KEY_CONFIG_DIR = "configDir"
  }
}
