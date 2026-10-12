package org.opengrimoire.ganbaruai.mobile.media

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.provider.DocumentsContract
import android.util.Log
import app.tauri.plugin.Channel
import app.tauri.plugin.JSObject
import org.json.JSONObject

/** Prefix of selected-tree media locators produced by the Rust Music queue. */
private const val SAF_LOCATOR_PREFIX = "ganbaru-saf:"

/** Typed interruption context for the native session owner. */
internal enum class NativeMusicInterruption(val wireValue: String) {
  ServiceStopped("service-stopped"),
  SourceAuthorityChanged("source-authority-changed"),
  SourceResolutionTimeout("source-resolution-timeout"),
}

/** Process-owned service transport. Queue policy and generation decisions remain in Rust. */
object NativeMusicSession {
  private val main = Handler(Looper.getMainLooper())
  @Volatile private var channel: Channel? = null
  private const val MAX_PENDING_EFFECTS = 64
  private val sourceWorker = newMusicSourceWorker()
  private val effects = NativeMusicEffectBuffer<MusicPlaybackService, JSONObject>(
    MAX_PENDING_EFFECTS,
    { action -> main.post { action() } },
    { owner, effect ->
      val deliveryId = effect.optLong("deliveryId", 0)
      val error = try {
        NativeMusicAuthority.requireCurrent(deliveryId)
        owner.applyNativeEffect(effect)
        null
      } catch (error: Exception) { error.message ?: "Android Music effect failed" }
      reportApplied(deliveryId, error)
    },
  )

  fun attach(context: Context, callback: Channel) {
    channel = callback
    if (!effects.beginStart()) return
    try { checkNotNull(context.applicationContext.startService(Intent(context.applicationContext, MusicPlaybackService::class.java))) { "Android Music service could not start" } }
    catch (error: Exception) { effects.failedStart(); throw error }
  }

  fun created(owner: MusicPlaybackService) {
    effects.created(owner)
  }

  fun destroyed(owner: MusicPlaybackService) {
    effects.destroyed(owner)
  }

  /** Called through a retained JavaVM/global class reference, without an Activity. */
  @JvmStatic
  fun isActive(): Boolean = effects.isActive()

  /** Called through a retained JavaVM/global class reference, without an Activity. */
  @JvmStatic
  fun dispatch(encoded: String): Boolean {
    if (channel == null || encoded.length > 64 * 1024) return false
    val effect = JSONObject(encoded)
    if (!NativeMusicAuthority.isDeliveryCurrent(effect.optLong("deliveryId", 0))) return false
    return effects.offer(effect)
  }

  /** Confirmation is false while the matching SDK call is still executing. */
  @JvmStatic
  fun cancelDelivery(deliveryId: Long): Boolean =
    deliveryId > 0 && effects.cancel { it.optLong("deliveryId", 0) == deliveryId }

  private fun reportApplied(deliveryId: Long, error: String?) {
    // Error text crosses JSON as valid UTF-16 and remains bounded in UTF-8.
    val bounded = error?.let { message ->
      val end = message.offsetByCodePoints(0, minOf(message.codePointCount(0, message.length), 1024))
      message.substring(0, end)
    }
    val target = channel
    if (target == null) { Log.e("GanbaruMusic", "Music execution $deliveryId has no acknowledgement channel"); return }
    try {
      target.send(JSObject().apply {
        put("type", "applied"); put("deliveryId", deliveryId); put("error", bounded ?: JSONObject.NULL)
      })
    } catch (error: Exception) {
      // The native waiter retains cancellation and a Stop retry after timeout.
      // A lost acknowledgement must not crash the main-thread service drain.
      Log.e("GanbaruMusic", "Music execution $deliveryId acknowledgement failed", error)
    }
  }

  fun reportObservation(observation: JSObject) {
    channel?.send(JSObject().apply { put("type", "observation"); put("observation", observation) })
  }

  fun reportControl(intent: JSObject) {
    channel?.send(JSObject().apply { put("type", "control"); put("intent", intent) })
  }

  internal fun reportUnavailable(sessionId: String, generation: Long, reason: NativeMusicInterruption = NativeMusicInterruption.ServiceStopped) {
    channel?.send(JSObject().apply {
      put("type", "unavailable"); put("sessionId", sessionId); put("generation", generation)
      put("reason", reason.wireValue)
    })
  }

  /** A destroyed service cannot create a replacement worker around stalled provider IO. */
  internal fun resolveSource(action: () -> Unit) { sourceWorker.execute(action) }

  /**
   * Resolves direct document URIs and selected-tree locators through the ContentResolver.
   * This is the single validated locator parser for playback and metadata reads.
   */
  fun resolveUri(context: Context, locator: String): Uri {
    val direct = Uri.parse(locator)
    if (direct.scheme == "content") return direct
    require(locator.startsWith(SAF_LOCATOR_PREFIX)) { "Android media requires a selected document" }
    val encoded = locator.removePrefix(SAF_LOCATOR_PREFIX)
    val separator = encoded.indexOf('#')
    require(separator > 0 && separator < encoded.lastIndex) { "Android media locator is invalid" }
    val tree = Uri.parse(Uri.decode(encoded.substring(0, separator)))
    val relative = Uri.decode(encoded.substring(separator + 1))
    val segments = relative.split('/')
    require(segments.none { it.isBlank() || it == "." || it == ".." || it.contains('\\') }) { "Android music path is invalid" }
    require(tree.scheme == "content" && context.contentResolver.persistedUriPermissions.any { it.uri == tree && it.isReadPermission }) { "Music folder access needs to be selected again" }
    var documentId = DocumentsContract.getTreeDocumentId(tree)
    for (segment in segments) {
      val childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(tree, documentId)
      var found: String? = null
      context.contentResolver.query(childrenUri, arrayOf(DocumentsContract.Document.COLUMN_DOCUMENT_ID, DocumentsContract.Document.COLUMN_DISPLAY_NAME), null, null, null)?.use { cursor ->
        while (cursor.moveToNext()) { if (cursor.getString(1) == segment) { found = cursor.getString(0); break } }
      }
      documentId = found ?: throw IllegalStateException("The selected media file has moved or was removed")
    }
    return DocumentsContract.buildDocumentUriUsingTree(tree, documentId)
  }
}
