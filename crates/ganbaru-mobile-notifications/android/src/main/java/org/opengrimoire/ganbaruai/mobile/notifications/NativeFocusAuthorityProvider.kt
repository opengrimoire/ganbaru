package org.opengrimoire.ganbaruai.mobile.notifications

import android.content.ContentProvider
import android.content.ContentValues
import android.content.Context
import android.database.Cursor
import android.net.Uri
import android.os.Binder
import android.os.Bundle
import android.os.Process

internal const val FOCUS_SCOPE_GENERATION = "generation"
internal const val FOCUS_SCOPE_REVISION = "revision"
internal const val FOCUS_SCOPE_PROCESS = "processNonce"
private const val AUTHORITY_SUFFIX = ".ganbaru.focus.authority"
private const val METHOD_CURRENT = "current"
private const val METHOD_PROCESS = "process"
private const val KEY_CURRENT = "current"

/** Read-only, app-private access to cached authority in the Rust hosting process. */
class NativeFocusAuthorityProvider : ContentProvider() {
  override fun onCreate(): Boolean = true

  override fun call(method: String, arg: String?, extras: Bundle?): Bundle {
    check(Binder.getCallingUid() == Process.myUid()) { "Focus authority is app-private" }
    val current = when (method) {
      METHOD_CURRENT -> {
        val scope = requireScope(extras)
        NativeFocusAuthority.currentOrUnavailable(scope.processNonce, scope.generation, scope.revision)
      }
      METHOD_PROCESS -> NativeFocusAuthority.processOrUnavailable(requireProcessScope(extras).processNonce)
      else -> error("Unknown Focus authority operation")
    }
    return Bundle().apply {
      putBoolean(KEY_CURRENT, current)
    }
  }

  override fun query(uri: Uri, projection: Array<out String>?, selection: String?, selectionArgs: Array<out String>?, sortOrder: String?): Cursor? = unsupported()
  override fun getType(uri: Uri): String? = unsupported()
  override fun insert(uri: Uri, values: ContentValues?): Uri? = unsupported()
  override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String>?): Int = unsupported()
  override fun update(uri: Uri, values: ContentValues?, selection: String?, selectionArgs: Array<out String>?): Int = unsupported()

  private fun <T> unsupported(): T = throw UnsupportedOperationException("Focus authority supports private read calls only")

  companion object {
    internal fun requireProcessScope(extras: Bundle?): NativeFocusProcessScope {
      require(extras != null && extras.containsKey(FOCUS_SCOPE_PROCESS)) { "Native Focus process scope is missing" }
      return NativeFocusProcessScope(extras.getLong(FOCUS_SCOPE_PROCESS))
    }

    internal fun requireScope(extras: Bundle?): NativeFocusDeliveryScope {
      require(extras != null && extras.containsKey(FOCUS_SCOPE_PROCESS) && extras.containsKey(FOCUS_SCOPE_GENERATION) && extras.containsKey(FOCUS_SCOPE_REVISION)) {
        "Native Focus delivery scope is missing"
      }
      return NativeFocusDeliveryScope(extras.getLong(FOCUS_SCOPE_PROCESS), extras.getLong(FOCUS_SCOPE_GENERATION), extras.getLong(FOCUS_SCOPE_REVISION))
    }

    /** Called under the Guardian publication lock, after any earlier operation. */
    internal fun isCurrent(context: Context, processNonce: Long, generation: Long, revision: Long): Boolean {
      val extras = Bundle().apply {
        putLong(FOCUS_SCOPE_PROCESS, processNonce)
        putLong(FOCUS_SCOPE_GENERATION, generation)
        putLong(FOCUS_SCOPE_REVISION, revision)
      }
      return read(context, METHOD_CURRENT, extras)
    }

    internal fun isProcessCurrent(context: Context, processNonce: Long): Boolean =
      read(context, METHOD_PROCESS, Bundle().apply { putLong(FOCUS_SCOPE_PROCESS, processNonce) })

    private fun read(context: Context, method: String, extras: Bundle): Boolean {
      val uri = Uri.Builder().scheme("content").authority(context.packageName + AUTHORITY_SUFFIX).build()
      val response = context.contentResolver.call(uri, method, null, extras)
        ?: error("Native Focus authority did not return a response")
      require(response.containsKey(KEY_CURRENT)) { "Native Focus authority response is invalid" }
      return response.getBoolean(KEY_CURRENT)
    }
  }
}
