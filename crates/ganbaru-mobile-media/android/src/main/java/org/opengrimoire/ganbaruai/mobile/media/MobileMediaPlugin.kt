package org.opengrimoire.ganbaruai.mobile.media

import android.app.Activity
import android.content.Intent
import android.media.MediaMetadataRetriever
import android.net.Uri
import android.os.Environment
import android.provider.DocumentsContract
import android.util.Base64
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Channel
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.util.Locale
import java.util.concurrent.atomic.AtomicBoolean

private const val MAX_SCANNED_ENTRIES = 20_000
private const val EXTERNAL_STORAGE_DOCUMENTS_AUTHORITY = "com.android.externalstorage.documents"

@InvokeArg
internal class AttachSessionArgs {
  lateinit var channel: Channel
}

private val supportedAudioExtensions = setOf(
  "aac", "aif", "aiff", "alac", "flac", "m4a", "mp3", "ogg", "opus", "wav", "wma",
)

private val embeddedArtworkExtensions = setOf(
  "aac", "aif", "aiff", "alac", "flac", "m4a", "mp3", "ogg", "opus", "wav",
)

@InvokeArg
internal class PathArgs {
  lateinit var path: String
}

@InvokeArg
internal class TreePickArgs {
  var maxFiles: Int = 0
  var maxDepth: Int = 0
}

@InvokeArg
internal class TreeScanArgs {
  lateinit var treeUri: String
  var maxFiles: Int = 0
  var maxDepth: Int = 0
}

@InvokeArg
internal class ArtworkArgs {
  lateinit var path: String
  var embedded: Boolean = false
  var maxBytes: Long = 0
}

private data class PendingTreePick(
  val maxFiles: Int,
  val maxDepth: Int,
)

private data class DocumentEntry(
  val documentId: String,
  val displayName: String,
  val mimeType: String,
  val flags: Int,
  val size: Long?,
  val modifiedAtMs: Long?,
)

private data class ScanBudget(
  val maxFiles: Int,
  val maxDepth: Int,
  var scannedEntries: Int = 0,
  var mediaFiles: Int = 0,
  var truncated: Boolean = false,
)

@TauriPlugin
class MobileMediaPlugin(private val activity: Activity) : Plugin(activity) {
  @Command
  fun attachSession(invoke: Invoke) {
    try {
      NativeMusicSession.attach(activity.applicationContext, invoke.parseArgs(AttachSessionArgs::class.java).channel)
      invoke.resolve()
    } catch (error: Exception) { invoke.reject("Attach native music session: ${error.message}") }
  }

  private var pendingTreePick: PendingTreePick? = null
  @Volatile private var pendingTreeCache: Pair<String, JSObject>? = null
  private val pendingArtworkPick = AtomicBoolean(false)

  @Command
  fun probe(invoke: Invoke) {
    val args = invoke.parseArgs(PathArgs::class.java)
    Thread {
      try {
        val uri = resolveMediaUri(args.path)
        val metadata = documentMetadata(uri)
        val media = readMediaMetadata(
          uri,
          metadata.displayName.substringBeforeLast('.', metadata.displayName),
          extension(metadata.displayName) in embeddedArtworkExtensions,
        )
        invoke.resolve(JSObject().apply {
          put("path", args.path)
          put("title", media.title)
          put("fileSizeBytes", metadata.size ?: 0L)
          put("extension", extension(metadata.displayName))
          put("mediaKind", if (media.hasVideo) "video" else "audio")
          put("playableStartMs", null)
        })
      } catch (error: Exception) {
        invoke.reject(error.message ?: "Failed to inspect selected media")
      }
    }.start()
  }

  @Command
  fun pickMediaTree(invoke: Invoke) {
    try {
      require(pendingTreePick == null) { "A music folder picker is already active" }
      val args = invoke.parseArgs(TreePickArgs::class.java)
      validateScanLimits(args.maxFiles, args.maxDepth)
      pendingTreePick = PendingTreePick(args.maxFiles, args.maxDepth)
      val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        addFlags(Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
        putExtra(DocumentsContract.EXTRA_INITIAL_URI, defaultMusicDirectoryUri())
      }
      startActivityForResult(invoke, intent, "pickMediaTreeResult")
    } catch (error: Exception) {
      pendingTreePick = null
      invoke.reject(error.message ?: "Failed to open music folder picker")
    }
  }

  private fun defaultMusicDirectoryUri(): Uri = DocumentsContract.buildDocumentUri(
    EXTERNAL_STORAGE_DOCUMENTS_AUTHORITY,
    "primary:${Environment.DIRECTORY_MUSIC}",
  )

  @ActivityCallback
  fun pickMediaTreeResult(invoke: Invoke, result: ActivityResult) {
    val pending = pendingTreePick
    pendingTreePick = null
    if (result.resultCode == Activity.RESULT_CANCELED) {
      invoke.resolve(JSObject().apply { put("tree", null) })
      return
    }
    val treeUri = result.data?.data
    if (result.resultCode != Activity.RESULT_OK || pending == null || treeUri == null) {
      invoke.reject("The selected music folder is unavailable")
      return
    }
    try {
      val grantedFlags = result.data?.flags?.and(
        Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION,
      ) ?: Intent.FLAG_GRANT_READ_URI_PERMISSION
      activity.contentResolver.takePersistableUriPermission(treeUri, grantedFlags)
    } catch (error: SecurityException) {
      invoke.reject("Android could not preserve access to the selected music folder")
      return
    }
    Thread {
      try {
        val tree = scanTree(treeUri, pending.maxFiles, pending.maxDepth)
        pendingTreeCache = treeUri.toString() to tree
        invoke.resolve(JSObject().apply {
          put("tree", tree)
        })
      } catch (error: Exception) {
        invoke.reject(error.message ?: "Failed to scan selected music folder")
      }
    }.start()
  }

  @Command
  fun scanMediaTree(invoke: Invoke) {
    val args = invoke.parseArgs(TreeScanArgs::class.java)
    Thread {
      try {
        validateScanLimits(args.maxFiles, args.maxDepth)
        val treeUri = Uri.parse(args.treeUri)
        require(treeUri.scheme == "content") { "Music folder access is invalid" }
        require(hasPersistedReadPermission(treeUri)) { "Music folder access needs to be selected again" }
        val cached = pendingTreeCache?.takeIf { it.first == treeUri.toString() }
        if (cached != null) pendingTreeCache = null
        invoke.resolve(cached?.second ?: scanTree(treeUri, args.maxFiles, args.maxDepth))
      } catch (error: Exception) {
        invoke.reject(error.message ?: "Failed to refresh selected music folder")
      }
    }.start()
  }

  @Command
  fun pickArtworkFile(invoke: Invoke) {
    if (!pendingArtworkPick.compareAndSet(false, true)) {
      invoke.reject("An artwork picker is already active")
      return
    }
    val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
      addCategory(Intent.CATEGORY_OPENABLE)
      type = "image/*"
      addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
      addFlags(Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
    }
    startActivityForResult(invoke, intent, "pickArtworkFileResult")
  }

  @ActivityCallback
  fun pickArtworkFileResult(invoke: Invoke, result: ActivityResult) {
    pendingArtworkPick.set(false)
    if (result.resultCode == Activity.RESULT_CANCELED) {
      invoke.resolve(JSObject().apply { put("uri", null) })
      return
    }
    val uri = result.data?.data
    if (result.resultCode != Activity.RESULT_OK || uri == null) {
      invoke.reject("The selected artwork is unavailable")
      return
    }
    try {
      activity.contentResolver.takePersistableUriPermission(
        uri,
        Intent.FLAG_GRANT_READ_URI_PERMISSION,
      )
      invoke.resolve(JSObject().apply { put("uri", uri.toString()) })
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Android could not preserve access to the selected artwork")
    }
  }

  @Command
  fun artworkDataUrl(invoke: Invoke) {
    val args = invoke.parseArgs(ArtworkArgs::class.java)
    Thread {
      try {
        require(args.maxBytes in 1..12L * 1024L * 1024L) { "Artwork size limit is invalid" }
        val uri = resolveMediaUri(args.path)
        val bytes = if (args.embedded) {
          val retriever = MediaMetadataRetriever()
          try {
            retriever.setDataSource(activity, uri)
            retriever.embeddedPicture
          } finally {
            retriever.release()
          }
        } else {
          readBoundedBytes(uri, args.maxBytes)
        }
        if (bytes == null) {
          invoke.resolve(JSObject().apply { put("dataUrl", null) })
          return@Thread
        }
        require(bytes.size.toLong() <= args.maxBytes) { "Artwork exceeds the display size limit" }
        val contentType = imageContentType(bytes)
          ?: throw IllegalArgumentException("Selected artwork is not a supported image")
        val encoded = Base64.encodeToString(bytes, Base64.NO_WRAP)
        invoke.resolve(JSObject().apply { put("dataUrl", "data:$contentType;base64,$encoded") })
      } catch (error: Exception) {
        invoke.reject(error.message ?: "Failed to load artwork")
      }
    }.start()
  }

  private fun validateScanLimits(maxFiles: Int, maxDepth: Int) {
    require(maxFiles in 1..5_000) { "Music scan file limit must be between 1 and 5000" }
    require(maxDepth in 1..64) { "Music scan depth limit must be between 1 and 64" }
  }

  private fun hasPersistedReadPermission(uri: Uri): Boolean =
    activity.contentResolver.persistedUriPermissions.any { permission ->
      permission.isReadPermission && permission.uri == uri
    }

  private fun scanTree(treeUri: Uri, maxFiles: Int, maxDepth: Int): JSObject {
    val rootId = DocumentsContract.getTreeDocumentId(treeUri)
    val rootUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, rootId)
    val rootName = documentMetadata(rootUri).displayName
    val budget = ScanBudget(maxFiles, maxDepth)
    val tracks = ArrayList<JSObject>()
    scanDirectory(treeUri, rootId, "", 0, budget, tracks)
    tracks.sortBy { it.getString("relativePath") }
    return JSObject().apply {
      put("treeUri", treeUri.toString())
      put("displayName", rootName)
      put("tracks", JSArray(tracks))
      put("truncated", budget.truncated)
    }
  }

  private fun scanDirectory(
    treeUri: Uri,
    documentId: String,
    relativeDirectory: String,
    depth: Int,
    budget: ScanBudget,
    tracks: MutableList<JSObject>,
  ) {
    if (depth > budget.maxDepth || budget.truncated) {
      budget.truncated = true
      return
    }
    val children = queryDocumentChildren(treeUri, documentId)
      .sortedWith(compareBy(String.CASE_INSENSITIVE_ORDER) { it.displayName })
    for (entry in children) {
      budget.scannedEntries += 1
      if (budget.scannedEntries > MAX_SCANNED_ENTRIES) {
        budget.truncated = true
        return
      }
      val relativePath = if (relativeDirectory.isEmpty()) {
        entry.displayName
      } else {
        "$relativeDirectory/${entry.displayName}"
      }
      if (entry.mimeType == DocumentsContract.Document.MIME_TYPE_DIR) {
        scanDirectory(treeUri, entry.documentId, relativePath, depth + 1, budget, tracks)
        if (budget.truncated) return
        continue
      }
      if (entry.flags and DocumentsContract.Document.FLAG_VIRTUAL_DOCUMENT != 0) continue
      if (!isSupportedAudio(entry.displayName, entry.mimeType)) continue
      if (budget.mediaFiles >= budget.maxFiles) {
        budget.truncated = true
        return
      }
      val uri = DocumentsContract.buildDocumentUriUsingTree(treeUri, entry.documentId)
      val metadata = readMediaMetadata(
        uri,
        entry.displayName.substringBeforeLast('.', entry.displayName),
        extension(entry.displayName) in embeddedArtworkExtensions,
      )
      tracks.add(JSObject().apply {
        put("uri", uri.toString())
        put("relativePath", relativePath)
        put("title", metadata.title)
        put("artist", metadata.artist)
        put("album", metadata.album)
        put("trackNumber", metadata.trackNumber)
        put("artworkUri", null)
        put("embeddedArtworkCandidate", metadata.embeddedArtworkCandidate)
        put("durationMs", metadata.durationMs)
        put("fileSizeBytes", entry.size)
        put("modifiedAtMs", entry.modifiedAtMs)
        put("mediaKind", "audio")
        put("mimeType", entry.mimeType)
      })
      budget.mediaFiles += 1
    }
  }

  private fun queryDocumentChildren(treeUri: Uri, parentDocumentId: String): List<DocumentEntry> {
    val childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, parentDocumentId)
    val projection = arrayOf(
      DocumentsContract.Document.COLUMN_DOCUMENT_ID,
      DocumentsContract.Document.COLUMN_DISPLAY_NAME,
      DocumentsContract.Document.COLUMN_MIME_TYPE,
      DocumentsContract.Document.COLUMN_FLAGS,
      DocumentsContract.Document.COLUMN_SIZE,
      DocumentsContract.Document.COLUMN_LAST_MODIFIED,
    )
    return activity.contentResolver.query(childrenUri, projection, null, null, null)?.use { cursor ->
      val entries = ArrayList<DocumentEntry>()
      while (cursor.moveToNext()) {
        entries.add(
          DocumentEntry(
            documentId = cursor.getString(0),
            displayName = cursor.getString(1) ?: continue,
            mimeType = cursor.getString(2) ?: "application/octet-stream",
            flags = cursor.getInt(3),
            size = cursor.getLong(4).takeUnless { cursor.isNull(4) },
            modifiedAtMs = cursor.getLong(5).takeUnless { cursor.isNull(5) },
          ),
        )
      }
      entries
    } ?: throw IllegalStateException("Android could not list the selected music folder")
  }

  private fun documentMetadata(uri: Uri): DocumentEntry {
    val projection = arrayOf(
      DocumentsContract.Document.COLUMN_DOCUMENT_ID,
      DocumentsContract.Document.COLUMN_DISPLAY_NAME,
      DocumentsContract.Document.COLUMN_MIME_TYPE,
      DocumentsContract.Document.COLUMN_FLAGS,
      DocumentsContract.Document.COLUMN_SIZE,
      DocumentsContract.Document.COLUMN_LAST_MODIFIED,
    )
    return activity.contentResolver.query(uri, projection, null, null, null)?.use { cursor ->
      check(cursor.moveToFirst()) { "Selected media is unavailable" }
      DocumentEntry(
        documentId = cursor.getString(0),
        displayName = cursor.getString(1) ?: "Media",
        mimeType = cursor.getString(2) ?: "application/octet-stream",
        flags = cursor.getInt(3),
        size = cursor.getLong(4).takeUnless { cursor.isNull(4) },
        modifiedAtMs = cursor.getLong(5).takeUnless { cursor.isNull(5) },
      )
    } ?: throw IllegalStateException("Selected media is unavailable")
  }

  private data class ExtractedMetadata(
    val title: String,
    val artist: String,
    val album: String,
    val trackNumber: Long?,
    val durationMs: Long?,
    val hasVideo: Boolean,
    val embeddedArtworkCandidate: Boolean,
  )

  private fun readMediaMetadata(
    uri: Uri,
    fallbackTitle: String,
    embeddedArtworkCandidate: Boolean,
  ): ExtractedMetadata {
    val retriever = MediaMetadataRetriever()
    return try {
      retriever.setDataSource(activity, uri)
      val title = retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_TITLE)
        ?.trim().takeUnless { it.isNullOrEmpty() } ?: fallbackTitle
      val artist = retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_ARTIST)?.trim().orEmpty()
      val album = retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_ALBUM)?.trim().orEmpty()
      val trackNumber = retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_CD_TRACK_NUMBER)
        ?.substringBefore('/')?.trim()?.toLongOrNull()
      val duration = retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_DURATION)?.toLongOrNull()
      val hasVideo = retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_HAS_VIDEO) == "yes"
      ExtractedMetadata(title, artist, album, trackNumber, duration, hasVideo, embeddedArtworkCandidate)
    } catch (_: Exception) {
      ExtractedMetadata(fallbackTitle, "", "", null, null, false, embeddedArtworkCandidate)
    } finally {
      retriever.release()
    }
  }

  private fun isSupportedAudio(displayName: String, mimeType: String): Boolean {
    if (mimeType.startsWith("audio/")) return true
    return extension(displayName)?.lowercase(Locale.ROOT) in supportedAudioExtensions
  }

  private fun extension(displayName: String): String? {
    val index = displayName.lastIndexOf('.')
    if (index < 0 || index == displayName.lastIndex) return null
    return displayName.substring(index + 1).lowercase(Locale.ROOT)
  }

  private fun readBoundedBytes(uri: Uri, maxBytes: Long): ByteArray {
    val stream = activity.contentResolver.openInputStream(uri)
      ?: throw IllegalStateException("Selected artwork could not be opened")
    return stream.use { input ->
      val output = java.io.ByteArrayOutputStream()
      val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
      var total = 0L
      while (true) {
        val count = input.read(buffer)
        if (count < 0) break
        total += count
        require(total <= maxBytes) { "Artwork exceeds the display size limit" }
        output.write(buffer, 0, count)
      }
      output.toByteArray()
    }
  }

  private fun imageContentType(bytes: ByteArray): String? = when {
    bytes.size >= 8 && bytes.copyOfRange(0, 8).contentEquals(
      byteArrayOf(0x89.toByte(), 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a),
    ) -> "image/png"
    bytes.size >= 3 && bytes[0] == 0xff.toByte() && bytes[1] == 0xd8.toByte() && bytes[2] == 0xff.toByte() -> "image/jpeg"
    bytes.size >= 6 && (String(bytes, 0, 6, Charsets.US_ASCII) == "GIF87a" || String(bytes, 0, 6, Charsets.US_ASCII) == "GIF89a") -> "image/gif"
    bytes.size >= 12 && String(bytes, 0, 4, Charsets.US_ASCII) == "RIFF" && String(bytes, 8, 4, Charsets.US_ASCII) == "WEBP" -> "image/webp"
    bytes.size >= 2 && bytes[0] == 'B'.code.toByte() && bytes[1] == 'M'.code.toByte() -> "image/bmp"
    bytes.size >= 12 && String(bytes, 4, 4, Charsets.US_ASCII) == "ftyp" && String(bytes, 8, 4, Charsets.US_ASCII) in setOf("avif", "avis") -> "image/avif"
    else -> null
  }

  /** Uses the playback session parser so metadata reads accept exactly the locators playback accepts. */
  private fun resolveMediaUri(locator: String): Uri = NativeMusicSession.resolveUri(activity, locator)
}
