package org.opengrimoire.ganbaru.mobile.documents

import android.app.Activity
import android.app.AlertDialog
import android.content.ComponentName
import android.content.ContentUris
import android.content.ContentValues
import android.content.Intent
import android.net.Uri
import android.os.Environment
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.provider.MediaStore
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.FileOutputStream
import java.io.InputStream
import java.nio.ByteBuffer
import java.nio.charset.CodingErrorAction

private const val DEFAULT_JSON_MIME_TYPE = "application/json"
private const val EXTERNAL_STORAGE_DOCUMENTS_AUTHORITY = "com.android.externalstorage.documents"
private const val PRIMARY_STORAGE_ROOT_DOCUMENT_ID = "primary:"

/** Rejection message of a request whose picked backup folder lost its access grant or was removed. */
internal const val BACKUP_FOLDER_UNAVAILABLE = "The backup folder is unavailable"

private fun primaryStorageRootUri(): Uri = DocumentsContract.buildDocumentUri(
  EXTERNAL_STORAGE_DOCUMENTS_AUTHORITY,
  PRIMARY_STORAGE_ROOT_DOCUMENT_ID,
)

private fun downloadsDirectoryUri(): Uri = DocumentsContract.buildDocumentUri(
  EXTERNAL_STORAGE_DOCUMENTS_AUTHORITY,
  "$PRIMARY_STORAGE_ROOT_DOCUMENT_ID${Environment.DIRECTORY_DOWNLOADS}",
)

private fun documentsDirectoryUri(): Uri = DocumentsContract.buildDocumentUri(
  EXTERNAL_STORAGE_DOCUMENTS_AUTHORITY,
  "$PRIMARY_STORAGE_ROOT_DOCUMENT_ID${Environment.DIRECTORY_DOCUMENTS}",
)

/** The MediaStore collection of non-media files in shared storage, which holds folders below Documents. */
private fun sharedFilesUri(): Uri = MediaStore.Files.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)

@InvokeArg
internal class PickUtf8DocumentArgs {
  var maxBytes: Long = 0
  var acceptedExtensions: List<String> = emptyList()
  var mimeTypes: List<String> = emptyList()
  var documentKind: String = "document"
}

@InvokeArg
internal class SaveUtf8DownloadArgs {
  lateinit var fileName: String
  lateinit var contents: String
  var maxBytes: Long = 0
  var acceptedExtensions: List<String> = emptyList()
  var mimeType: String = DEFAULT_JSON_MIME_TYPE
  var documentKind: String = "document"
}

@InvokeArg
internal class PickDocumentToPathArgs {
  lateinit var destinationPath: String
  var maxBytes: Long = 0
  var acceptedExtensions: List<String> = emptyList()
  var mimeTypes: List<String> = emptyList()
  var documentKind: String = "document"
}

@InvokeArg
internal class SaveFileDownloadArgs {
  lateinit var sourcePath: String
  lateinit var fileName: String
  var maxBytes: Long = 0
  var acceptedExtensions: List<String> = emptyList()
  var mimeType: String = "application/octet-stream"
  var documentKind: String = "document"
}

@InvokeArg
internal class PickVaultTreeToPathArgs {
  lateinit var destinationPath: String
  var maxFiles: Int = 0
  var maxBytes: Long = 0
  var maxDepth: Int = 0
}

internal data class VaultTreeCopyLimits(
  val maxFiles: Int,
  val maxBytes: Long,
  val maxDepth: Int,
)

internal class VaultTreeCopyBudget(private val limits: VaultTreeCopyLimits) {
  private var entryCount = 0
  private var byteCount = 0L

  fun enter(depth: Int) {
    require(depth <= limits.maxDepth) { "Selected folder is nested too deeply" }
    entryCount += 1
    require(entryCount <= limits.maxFiles) { "Selected folder contains too many entries" }
  }

  fun addBytes(count: Int) {
    byteCount += count
    require(byteCount <= limits.maxBytes) { "Selected folder exceeds the import size limit" }
  }
}

internal object VaultTreePaths {
  fun validateDisplayName(name: String): String {
    require(name.isNotBlank()) { "Selected folder contains an unnamed entry" }
    require(name != "." && name != "..") { "Selected folder contains an invalid entry name" }
    require('/' !in name && '\\' !in name && '\u0000' !in name) {
      "Selected folder contains an invalid entry name"
    }
    return name
  }

  fun privateEmptyDestination(dataRoot: File, requestedPath: String): File {
    require(requestedPath.isNotBlank()) { "Import destination is required" }
    val canonicalRoot = dataRoot.canonicalFile
    val destination = File(requestedPath).canonicalFile
    require(destination.path.startsWith(canonicalRoot.path + File.separator)) {
      "Import destination must be inside app-private storage"
    }
    require(!destination.exists()) { "Import destination already exists" }
    val parent = destination.parentFile
      ?: throw IllegalArgumentException("Import destination has no parent")
    require(parent.isDirectory) { "Import destination parent is unavailable" }
    return destination
  }
}

internal object PrivateTransferPaths {
  fun unusedDestination(dataRoot: File, requestedPath: String): File {
    require(requestedPath.isNotBlank()) { "Import destination is required" }
    val canonicalRoot = dataRoot.canonicalFile
    val destination = File(requestedPath).canonicalFile
    require(destination.path.startsWith(canonicalRoot.path + File.separator)) {
      "Import destination must be inside app-private storage"
    }
    require(!destination.exists()) { "Import destination already exists" }
    require(destination.parentFile?.isDirectory == true) { "Import destination parent is unavailable" }
    return destination
  }

  fun readableSource(dataRoot: File, requestedPath: String): File {
    require(requestedPath.isNotBlank()) { "Export source is required" }
    val canonicalRoot = dataRoot.canonicalFile
    val source = File(requestedPath).canonicalFile
    require(source.path.startsWith(canonicalRoot.path + File.separator)) {
      "Export source must be inside app-private storage"
    }
    require(source.isFile) { "Export source is unavailable" }
    return source
  }
}

private data class DocumentEntry(
  val documentId: String,
  val displayName: String,
  val mimeType: String,
  val flags: Int,
)

private data class PendingVaultTreeCopy(
  val destination: File,
  val limits: VaultTreeCopyLimits,
)

private class PendingDocumentSave(
  val bytes: ByteArray,
)

private data class PendingDocumentRead(
  val maxBytes: Long,
  val acceptedExtensions: List<String>,
  val documentKind: String,
)

private data class PendingDocumentCopy(
  val destination: File,
  val maxBytes: Long,
  val acceptedExtensions: List<String>,
  val documentKind: String,
)

internal object DocumentTextCodec {
  /**
   * Whether a picker result names content served by another app. File paths and this app's own
   * providers are refused, so a picker cannot make the app read its private storage.
   */
  fun isAcceptedPickerResult(scheme: String?, authority: String?, providerPackage: String?, ownPackage: String): Boolean =
    scheme == "content" && !authority.isNullOrEmpty() && providerPackage != ownPackage

  fun hasAllowedExtension(fileName: String, acceptedExtensions: List<String>): Boolean {
    val extension = fileName.substringAfterLast('.', missingDelimiterValue = "").lowercase()
    return extension.isNotEmpty() && acceptedExtensions.any { allowed ->
      extension == allowed.trim().removePrefix(".").lowercase()
    }
  }

  fun readUtf8(input: InputStream, maxBytes: Long): String {
    require(maxBytes > 0) { "Document size limit must be positive" }
    val output = ByteArrayOutputStream()
    val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
    var total = 0L

    input.use { stream ->
      while (true) {
        val count = stream.read(buffer)
        if (count < 0) break
        total += count
        if (total > maxBytes) {
          throw IllegalArgumentException("Selected document exceeds the size limit")
        }
        output.write(buffer, 0, count)
      }
    }

    return Charsets.UTF_8.newDecoder()
      .onMalformedInput(CodingErrorAction.REPORT)
      .onUnmappableCharacter(CodingErrorAction.REPORT)
      .decode(ByteBuffer.wrap(output.toByteArray()))
      .toString()
  }
}

@TauriPlugin
class MobileDocumentsPlugin(private val activity: Activity) : Plugin(activity) {
  private val pendingDocumentRead = PendingPicker<PendingDocumentRead>("A document picker is already active")
  private val pendingVaultTreeCopy = PendingPicker<PendingVaultTreeCopy>("A folder import is already active")
  private val pendingDocumentCopy = PendingPicker<PendingDocumentCopy>("A document import is already active")
  private val pendingDocumentSave = PendingPicker<PendingDocumentSave>("A document save is already active")
  private val pendingBackupTree = PendingPicker<Unit>("A folder picker is already active")

  @Command
  fun pickVaultTreeToPath(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(PickVaultTreeToPathArgs::class.java)
      require(args.maxFiles > 0) { "Folder entry limit must be positive" }
      require(args.maxBytes > 0) { "Folder size limit must be positive" }
      require(args.maxDepth > 0) { "Folder depth limit must be positive" }
      val destination = VaultTreePaths.privateEmptyDestination(
        File(activity.applicationInfo.dataDir),
        args.destinationPath,
      )
      pendingVaultTreeCopy.begin(invoke.id, PendingVaultTreeCopy(
        destination,
        VaultTreeCopyLimits(args.maxFiles, args.maxBytes, args.maxDepth),
      ))
      val pickerIntent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        putExtra(DocumentsContract.EXTRA_INITIAL_URI, primaryStorageRootUri())
      }
      startActivityForResult(invoke, pickerIntent, "pickVaultTreeToPathResult")
    } catch (error: Exception) {
      pendingVaultTreeCopy.release(invoke.id)
      invoke.reject(error.message ?: "Failed to open folder picker")
    }
  }

  @ActivityCallback
  fun pickVaultTreeToPathResult(invoke: Invoke, result: ActivityResult) {
    val pending = pendingVaultTreeCopy.take(invoke.id)
    if (result.resultCode == Activity.RESULT_CANCELED) {
      invoke.resolve(JSObject().apply { put("displayName", null) })
      return
    }
    if (result.resultCode != Activity.RESULT_OK || pending == null) {
      invoke.reject("Failed to select folder")
      return
    }
    val treeUri = result.data?.data
    if (treeUri == null) {
      invoke.reject("The selected folder is unavailable")
      return
    }

    Thread {
      try {
        check(pending.destination.mkdir()) { "Android could not create the import staging folder" }
        val rootId = DocumentsContract.getTreeDocumentId(treeUri)
        val rootUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, rootId)
        val displayName = resolveDocumentDisplayName(rootUri)
          ?: throw IllegalArgumentException("The selected folder has no name")
        copyDocumentDirectory(
          treeUri,
          rootId,
          pending.destination,
          0,
          VaultTreeCopyBudget(pending.limits),
        )
        invoke.resolve(JSObject().apply { put("displayName", displayName) })
      } catch (error: Exception) {
        pending.destination.deleteRecursively()
        invoke.reject(error.message ?: "Failed to import selected folder")
      }
    }.start()
  }

  private fun copyDocumentDirectory(
    treeUri: Uri,
    documentId: String,
    destination: File,
    depth: Int,
    budget: VaultTreeCopyBudget,
  ) {
    budget.enter(depth)
    for (entry in queryDocumentChildren(treeUri, documentId)) {
      val name = VaultTreePaths.validateDisplayName(entry.displayName)
      val child = File(destination, name)
      require(child.canonicalFile.parentFile == destination.canonicalFile) {
        "Selected folder contains an unsafe entry path"
      }
      require(!child.exists()) { "Selected folder contains duplicate entry names" }
      if (entry.mimeType == DocumentsContract.Document.MIME_TYPE_DIR) {
        check(child.mkdir()) { "Android could not create an imported directory" }
        copyDocumentDirectory(treeUri, entry.documentId, child, depth + 1, budget)
      } else {
        require(entry.flags and DocumentsContract.Document.FLAG_VIRTUAL_DOCUMENT == 0) {
          "Selected folder contains an unsupported virtual document"
        }
        copyDocumentFile(treeUri, entry.documentId, child, depth + 1, budget)
      }
    }
  }

  private fun copyDocumentFile(
    treeUri: Uri,
    documentId: String,
    destination: File,
    depth: Int,
    budget: VaultTreeCopyBudget,
  ) {
    budget.enter(depth)
    val documentUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, documentId)
    val input = activity.contentResolver.openInputStream(documentUri)
      ?: throw IllegalStateException("Android could not open an imported document")
    input.use { source ->
      FileOutputStream(destination).use { target ->
        val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
        while (true) {
          val count = source.read(buffer)
          if (count < 0) break
          budget.addBytes(count)
          target.write(buffer, 0, count)
        }
        target.flush()
        target.fd.sync()
      }
    }
  }

  private fun queryDocumentChildren(treeUri: Uri, parentDocumentId: String): List<DocumentEntry> {
    val childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, parentDocumentId)
    val projection = arrayOf(
      DocumentsContract.Document.COLUMN_DOCUMENT_ID,
      DocumentsContract.Document.COLUMN_DISPLAY_NAME,
      DocumentsContract.Document.COLUMN_MIME_TYPE,
      DocumentsContract.Document.COLUMN_FLAGS,
    )
    return activity.contentResolver.query(childrenUri, projection, null, null, null)?.use { cursor ->
      val entries = ArrayList<DocumentEntry>()
      while (cursor.moveToNext()) {
        entries.add(
          DocumentEntry(
            documentId = cursor.getString(0),
            displayName = cursor.getString(1),
            mimeType = cursor.getString(2),
            flags = cursor.getInt(3),
          ),
        )
      }
      entries
    } ?: throw IllegalStateException("Android could not list the selected folder")
  }

  private fun resolveDocumentDisplayName(uri: Uri): String? {
    val projection = arrayOf(DocumentsContract.Document.COLUMN_DISPLAY_NAME)
    return activity.contentResolver.query(uri, projection, null, null, null)?.use { cursor ->
      if (!cursor.moveToFirst()) return@use null
      cursor.getString(0)
    }
  }

  @Command
  fun pickUtf8Document(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(PickUtf8DocumentArgs::class.java)
      require(args.maxBytes > 0) { "Document size limit must be positive" }
      require(args.acceptedExtensions.isNotEmpty()) { "At least one document extension is required" }
      require(args.mimeTypes.isNotEmpty()) { "At least one document MIME type is required" }
      require(args.documentKind.isNotBlank()) { "Document kind is required" }
      pendingDocumentRead.begin(
        invoke.id,
        PendingDocumentRead(args.maxBytes, args.acceptedExtensions, args.documentKind.trim()),
      )

      val pickerIntent = Intent(Intent.ACTION_GET_CONTENT).apply {
        addCategory(Intent.CATEGORY_OPENABLE)
        type = if (args.mimeTypes.size == 1) args.mimeTypes.single() else "*/*"
        if (args.mimeTypes.size > 1) {
          putExtra(Intent.EXTRA_MIME_TYPES, args.mimeTypes.toTypedArray())
        }
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
      }
      val providers = activity.packageManager
        .queryIntentActivities(pickerIntent, 0)
        .distinctBy { candidate ->
          candidate.activityInfo.packageName to candidate.activityInfo.name
        }
        .sortedBy { candidate ->
          candidate.loadLabel(activity.packageManager).toString().lowercase()
        }
      require(providers.isNotEmpty()) { "No document provider is installed" }

      if (providers.size == 1) {
        startProvider(invoke, pickerIntent, providers.single())
        return
      }

      val labels = providers
        .map { candidate -> candidate.loadLabel(activity.packageManager).toString() }
        .toTypedArray()
      AlertDialog.Builder(activity)
        .setItems(labels) { dialog, index ->
          try {
            startProvider(invoke, pickerIntent, providers[index])
          } catch (error: Exception) {
            pendingDocumentRead.release(invoke.id)
            invoke.reject(error.message ?: "Failed to open document provider")
          }
          dialog.dismiss()
        }
        .setOnCancelListener {
          pendingDocumentRead.release(invoke.id)
          invoke.resolve(JSObject().apply { put("contents", null) })
        }
        .show()
    } catch (error: Exception) {
      pendingDocumentRead.release(invoke.id)
      invoke.reject(error.message ?: "Failed to open document picker")
    }
  }

  private fun startProvider(
    invoke: Invoke,
    pickerIntent: Intent,
    provider: android.content.pm.ResolveInfo,
  ) {
    pickerIntent.component = ComponentName(
      provider.activityInfo.packageName,
      provider.activityInfo.name,
    )
    startActivityForResult(invoke, pickerIntent, "pickUtf8DocumentResult")
  }

  @ActivityCallback
  fun pickUtf8DocumentResult(invoke: Invoke, result: ActivityResult) {
    val pending = pendingDocumentRead.take(invoke.id)

    if (result.resultCode == Activity.RESULT_CANCELED) {
      invoke.resolve(JSObject().apply { put("contents", null) })
      return
    }
    if (result.resultCode != Activity.RESULT_OK || pending == null) {
      invoke.reject("Failed to select document")
      return
    }
    val uri = result.data?.data
    if (uri == null) {
      invoke.reject("The selected document is unavailable")
      return
    }

    Thread {
      try {
        requireForeignPickerResult(uri)
        val displayName = resolveOpenableDisplayName(uri)
          ?: throw IllegalArgumentException("The selected document has no file name")
        require(DocumentTextCodec.hasAllowedExtension(displayName, pending.acceptedExtensions)) {
          "Select a supported ${pending.documentKind} file"
        }
        val stream = activity.contentResolver.openInputStream(uri)
          ?: throw IllegalStateException("The selected document could not be opened")
        val contents = DocumentTextCodec.readUtf8(stream, pending.maxBytes)
        invoke.resolve(JSObject().apply { put("contents", contents) })
      } catch (error: Exception) {
        invoke.reject(error.message ?: "Failed to read selected document")
      }
    }.start()
  }

  @Command
  fun pickDocumentToPath(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(PickDocumentToPathArgs::class.java)
      require(args.maxBytes > 0) { "Document size limit must be positive" }
      require(args.mimeTypes.isNotEmpty()) { "At least one document MIME type is required" }
      require(args.documentKind.isNotBlank()) { "Document kind is required" }
      val destination = PrivateTransferPaths.unusedDestination(
        File(activity.applicationInfo.dataDir),
        args.destinationPath,
      )
      pendingDocumentCopy.begin(invoke.id, PendingDocumentCopy(
        destination,
        args.maxBytes,
        args.acceptedExtensions,
        args.documentKind.trim(),
      ))
      val pickerIntent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
        addCategory(Intent.CATEGORY_OPENABLE)
        type = if (args.mimeTypes.size == 1) args.mimeTypes.single() else "*/*"
        if (args.mimeTypes.size > 1) putExtra(Intent.EXTRA_MIME_TYPES, args.mimeTypes.toTypedArray())
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        putExtra(DocumentsContract.EXTRA_INITIAL_URI, downloadsDirectoryUri())
      }
      startActivityForResult(invoke, pickerIntent, "pickDocumentToPathResult")
    } catch (error: Exception) {
      pendingDocumentCopy.release(invoke.id)
      invoke.reject(error.message ?: "Failed to open document picker")
    }
  }

  @ActivityCallback
  fun pickDocumentToPathResult(invoke: Invoke, result: ActivityResult) {
    val pending = pendingDocumentCopy.take(invoke.id)
    if (result.resultCode == Activity.RESULT_CANCELED) {
      invoke.resolve(JSObject().apply { put("displayName", null) })
      return
    }
    if (result.resultCode != Activity.RESULT_OK || pending == null) {
      invoke.reject("Failed to select document")
      return
    }
    val uri = result.data?.data
    if (uri == null) {
      invoke.reject("The selected document is unavailable")
      return
    }

    Thread {
      try {
        requireForeignPickerResult(uri)
        val displayName = resolveOpenableDisplayName(uri)
          ?: throw IllegalArgumentException("The selected document has no file name")
        // An empty list accepts any name; the caller then checks the copied bytes itself.
        require(
          pending.acceptedExtensions.isEmpty() ||
            DocumentTextCodec.hasAllowedExtension(displayName, pending.acceptedExtensions)
        ) {
          "Select a supported ${pending.documentKind} file"
        }
        val source = activity.contentResolver.openInputStream(uri)
          ?: throw IllegalStateException("The selected document could not be opened")
        copyBounded(source, pending.destination, pending.maxBytes)
        invoke.resolve(JSObject().apply { put("displayName", displayName) })
      } catch (error: Exception) {
        pending.destination.delete()
        invoke.reject(error.message ?: "Failed to import selected document")
      }
    }.start()
  }

  @Command
  fun saveUtf8Download(invoke: Invoke) {
    val args = try {
      invoke.parseArgs(SaveUtf8DownloadArgs::class.java)
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Invalid download request")
      return
    }

    Thread {
      var uri: Uri? = null
      try {
        val bytes = args.contents.toByteArray(Charsets.UTF_8)
        require(args.maxBytes > 0) { "Document size limit must be positive" }
        require(bytes.size.toLong() <= args.maxBytes) { "${args.documentKind} export exceeds the size limit" }
        validateDownloadMetadata(
          args.fileName,
          args.acceptedExtensions,
          args.mimeType,
          args.documentKind,
        )
        val createdUri = createPendingDownload(args.fileName, args.mimeType)
        uri = createdUri

        activity.contentResolver.openOutputStream(createdUri, "w").use { output ->
          requireNotNull(output) { "Android could not open the download" }
          output.write(bytes)
          output.flush()
        }

        publishDownload(createdUri)
        val displayName = resolveDisplayName(createdUri) ?: args.fileName
        invoke.resolve(JSObject().apply { put("displayName", displayName) })
      } catch (error: Exception) {
        uri?.let { activity.contentResolver.delete(it, null, null) }
        invoke.reject(error.message ?: "Failed to save download")
      }
    }.start()
  }

  /**
   * Saves bounded UTF-8 text where the person chooses in the system picker, so the text never
   * lands in a shared collection without that choice.
   */
  @Command
  fun saveUtf8Document(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(SaveUtf8DownloadArgs::class.java)
      val bytes = args.contents.toByteArray(Charsets.UTF_8)
      require(args.maxBytes > 0) { "Document size limit must be positive" }
      require(bytes.size.toLong() <= args.maxBytes) { "${args.documentKind} export exceeds the size limit" }
      validateDownloadMetadata(
        args.fileName,
        args.acceptedExtensions,
        args.mimeType,
        args.documentKind,
      )
      pendingDocumentSave.begin(invoke.id, PendingDocumentSave(bytes))
      val pickerIntent = Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
        addCategory(Intent.CATEGORY_OPENABLE)
        type = args.mimeType
        putExtra(Intent.EXTRA_TITLE, args.fileName)
        putExtra(DocumentsContract.EXTRA_INITIAL_URI, downloadsDirectoryUri())
      }
      startActivityForResult(invoke, pickerIntent, "saveUtf8DocumentResult")
    } catch (error: Exception) {
      pendingDocumentSave.release(invoke.id)
      invoke.reject(error.message ?: "Failed to open document picker")
    }
  }

  @ActivityCallback
  fun saveUtf8DocumentResult(invoke: Invoke, result: ActivityResult) {
    val pending = pendingDocumentSave.take(invoke.id)
    if (result.resultCode == Activity.RESULT_CANCELED) {
      invoke.resolve(JSObject().apply { put("saved", false) })
      return
    }
    if (result.resultCode != Activity.RESULT_OK || pending == null) {
      invoke.reject("Failed to save document")
      return
    }
    val uri = result.data?.data
    if (uri == null) {
      invoke.reject("The selected document is unavailable")
      return
    }

    Thread {
      try {
        requireForeignPickerResult(uri)
        activity.contentResolver.openOutputStream(uri, "wt").use { output ->
          requireNotNull(output) { "Android could not open the document" }
          output.write(pending.bytes)
          output.flush()
        }
        invoke.resolve(JSObject().apply { put("saved", true) })
      } catch (error: Exception) {
        try {
          DocumentsContract.deleteDocument(activity.contentResolver, uri)
        } catch (cleanup: Exception) {
          // A partial document Android cannot delete stays where the person chose it, and the
          // rejection below still reports the failed save.
        }
        invoke.reject(error.message ?: "Failed to save document")
      }
    }.start()
  }

  @Command
  fun saveFileDownload(invoke: Invoke) {
    val args = try {
      invoke.parseArgs(SaveFileDownloadArgs::class.java)
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Invalid download request")
      return
    }

    Thread {
      var uri: Uri? = null
      try {
        val source = PrivateTransferPaths.readableSource(
          File(activity.applicationInfo.dataDir),
          args.sourcePath,
        )
        require(args.maxBytes > 0) { "Document size limit must be positive" }
        validateDownloadMetadata(
          args.fileName,
          args.acceptedExtensions,
          args.mimeType,
          args.documentKind,
        )
        require(source.length() <= args.maxBytes) { "${args.documentKind} export exceeds the size limit" }
        val createdUri = createPendingDownload(args.fileName, args.mimeType)
        uri = createdUri
        source.inputStream().use { input ->
          activity.contentResolver.openOutputStream(createdUri, "w").use { output ->
            requireNotNull(output) { "Android could not open the download" }
            input.copyTo(output)
            output.flush()
          }
        }
        publishDownload(createdUri)
        val displayName = resolveDisplayName(createdUri) ?: args.fileName
        invoke.resolve(JSObject().apply { put("displayName", displayName) })
      } catch (error: Exception) {
        uri?.let { activity.contentResolver.delete(it, null, null) }
        invoke.reject(error.message ?: "Failed to save download")
      }
    }.start()
  }

  /** Lets the user pick a folder for automatic backups and keeps write access to it across restarts. */
  @Command
  fun pickBackupTree(invoke: Invoke) {
    try {
      pendingBackupTree.begin(invoke.id, Unit)
      val pickerIntent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
        addFlags(
          Intent.FLAG_GRANT_READ_URI_PERMISSION
            or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
            or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION,
        )
        putExtra(DocumentsContract.EXTRA_INITIAL_URI, documentsDirectoryUri())
      }
      startActivityForResult(invoke, pickerIntent, "pickBackupTreeResult")
    } catch (error: Exception) {
      pendingBackupTree.release(invoke.id)
      invoke.reject(error.message ?: "Failed to open folder picker")
    }
  }

  @ActivityCallback
  fun pickBackupTreeResult(invoke: Invoke, result: ActivityResult) {
    val pending = pendingBackupTree.take(invoke.id)
    if (result.resultCode == Activity.RESULT_CANCELED) {
      invoke.resolve(JSObject().apply { put("treeUri", null) })
      return
    }
    val treeUri = result.data?.data
    if (result.resultCode != Activity.RESULT_OK || pending == null || treeUri == null) {
      invoke.reject("Failed to select folder")
      return
    }
    try {
      requireForeignPickerResult(treeUri)
      activity.contentResolver.takePersistableUriPermission(
        treeUri,
        Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION,
      )
      val rootUri = DocumentsContract.buildDocumentUriUsingTree(
        treeUri,
        DocumentsContract.getTreeDocumentId(treeUri),
      )
      val displayName = resolveDocumentDisplayName(rootUri)
        ?: throw IllegalArgumentException("The selected folder has no name")
      invoke.resolve(JSObject().apply {
        put("treeUri", treeUri.toString())
        put("displayName", displayName)
      })
    } catch (error: Exception) {
      invoke.reject(error.message ?: "The selected folder is unavailable")
    }
  }

  /** Whether the app still holds write access to the picked backup folder [BackupTreeArgs.treeUri]. */
  @Command
  fun backupTreeAvailable(invoke: Invoke) {
    val args = try {
      invoke.parseArgs(BackupTreeArgs::class.java)
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Invalid backup folder request")
      return
    }
    Thread {
      val available = try {
        val folder = BackupFolders.parse(args.treeUri, null, Environment.DIRECTORY_DOCUMENTS)
        backupTreeRoot(folder as BackupFolder.Tree) != null
      } catch (_: Exception) {
        false
      }
      invoke.resolve(JSObject().apply { put("available", available) })
    }.start()
  }

  /** Copies an app-private file into the automatic backup folder. */
  @Command
  fun saveFileToBackupFolder(invoke: Invoke) {
    val args = try {
      invoke.parseArgs(SaveFileToBackupFolderArgs::class.java)
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Invalid backup folder request")
      return
    }
    Thread {
      var created: Uri? = null
      try {
        val folder = BackupFolders.parse(args.treeUri, args.documentsSubdirectory, Environment.DIRECTORY_DOCUMENTS)
        val source = PrivateTransferPaths.readableSource(File(activity.applicationInfo.dataDir), args.sourcePath)
        require(args.maxBytes > 0) { "Document size limit must be positive" }
        validateDownloadMetadata(args.fileName, args.acceptedExtensions, args.mimeType, args.documentKind)
        require(source.length() <= args.maxBytes) { "${args.documentKind} exceeds the size limit" }
        val target = when (folder) {
          is BackupFolder.Documents ->
            createPendingDownload(args.fileName, args.mimeType, folder.relativePath, sharedFilesUri())
          is BackupFolder.Tree -> {
            val root = backupTreeRoot(folder) ?: throw IllegalStateException(BACKUP_FOLDER_UNAVAILABLE)
            DocumentsContract.createDocument(activity.contentResolver, root, args.mimeType, args.fileName)
              ?: throw IllegalStateException("Android could not create the backup file")
          }
        }
        created = target
        source.inputStream().use { input ->
          activity.contentResolver.openOutputStream(target, "w").use { output ->
            requireNotNull(output) { "Android could not open the backup file" }
            input.copyTo(output)
            output.flush()
          }
        }
        val displayName = when (folder) {
          is BackupFolder.Documents -> {
            publishDownload(target)
            resolveDisplayName(target)
          }
          is BackupFolder.Tree -> resolveDocumentDisplayName(target)
        } ?: args.fileName
        invoke.resolve(JSObject().apply { put("displayName", displayName) })
      } catch (error: Exception) {
        created?.let { uri ->
          try {
            if (DocumentsContract.isDocumentUri(activity, uri)) {
              DocumentsContract.deleteDocument(activity.contentResolver, uri)
            } else {
              activity.contentResolver.delete(uri, null, null)
            }
          } catch (_: Exception) {
          }
        }
        invoke.reject(error.message ?: "Failed to save the backup file")
      }
    }.start()
  }

  /** Lists the names of the files in the automatic backup folder that start with the request's prefix. */
  @Command
  fun listBackupFolderFiles(invoke: Invoke) {
    val args = try {
      invoke.parseArgs(BackupFolderFilesArgs::class.java)
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Invalid backup folder request")
      return
    }
    Thread {
      try {
        val folder = BackupFolders.parse(args.treeUri, args.documentsSubdirectory, Environment.DIRECTORY_DOCUMENTS)
        val prefix = BackupFolders.validatePrefix(args.prefix)
        val names = backupFolderFiles(folder, prefix).map { it.first }
        invoke.resolve(JSObject().apply { put("names", JSArray(names)) })
      } catch (error: Exception) {
        invoke.reject(error.message ?: "Failed to list the backup folder")
      }
    }.start()
  }

  /** Deletes the requested files of the automatic backup folder that start with the request's prefix. */
  @Command
  fun deleteBackupFolderFiles(invoke: Invoke) {
    val args = try {
      invoke.parseArgs(BackupFolderFilesArgs::class.java)
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Invalid backup folder request")
      return
    }
    Thread {
      try {
        val folder = BackupFolders.parse(args.treeUri, args.documentsSubdirectory, Environment.DIRECTORY_DOCUMENTS)
        val prefix = BackupFolders.validatePrefix(args.prefix)
        val files = backupFolderFiles(folder, prefix)
        val deletable = BackupFolders.deletable(files.map { it.first }, prefix, args.names).toSet()
        var deleted = 0
        for ((name, uri) in files) {
          if (name !in deletable) continue
          val removed = when (folder) {
            is BackupFolder.Documents -> activity.contentResolver.delete(uri, null, null) > 0
            is BackupFolder.Tree -> DocumentsContract.deleteDocument(activity.contentResolver, uri)
          }
          if (removed) deleted += 1
        }
        invoke.resolve(JSObject().apply { put("deleted", deleted) })
      } catch (error: Exception) {
        invoke.reject(error.message ?: "Failed to delete old backups")
      }
    }.start()
  }

  /** The root document of [folder] while the app holds persistent write access to it, or null. */
  private fun backupTreeRoot(folder: BackupFolder.Tree): Uri? {
    val treeUri = Uri.parse(folder.uri)
    val granted = activity.contentResolver.persistedUriPermissions.any { permission ->
      permission.uri == treeUri && permission.isWritePermission
    }
    if (!granted) return null
    val rootUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, DocumentsContract.getTreeDocumentId(treeUri))
    return try {
      resolveDocumentDisplayName(rootUri)?.let { rootUri }
    } catch (_: Exception) {
      null
    }
  }

  /** The regular files of [folder] whose names start with [prefix], with the URI each is deleted through. */
  private fun backupFolderFiles(folder: BackupFolder, prefix: String): List<Pair<String, Uri>> = when (folder) {
    is BackupFolder.Documents -> {
      val projection = arrayOf(MediaStore.MediaColumns._ID, MediaStore.MediaColumns.DISPLAY_NAME)
      val selection =
        "${MediaStore.MediaColumns.RELATIVE_PATH} = ? AND ${MediaStore.MediaColumns.DISPLAY_NAME} LIKE ? ESCAPE '\\'"
      val selectionArgs = arrayOf(folder.relativePath, BackupFolders.likePattern(prefix))
      activity.contentResolver.query(
        sharedFilesUri(),
        projection,
        selection,
        selectionArgs,
        null,
      )?.use { cursor ->
        val files = ArrayList<Pair<String, Uri>>()
        while (cursor.moveToNext()) {
          val name: String? = cursor.getString(1)
          if (name != null && name.startsWith(prefix)) {
            files.add(name to ContentUris.withAppendedId(sharedFilesUri(), cursor.getLong(0)))
          }
        }
        files
      } ?: throw IllegalStateException("Android could not list the backup folder")
    }
    is BackupFolder.Tree -> {
      backupTreeRoot(folder) ?: throw IllegalStateException(BACKUP_FOLDER_UNAVAILABLE)
      val treeUri = Uri.parse(folder.uri)
      queryDocumentChildren(treeUri, DocumentsContract.getTreeDocumentId(treeUri))
        .filter { entry ->
          entry.mimeType != DocumentsContract.Document.MIME_TYPE_DIR && entry.displayName.startsWith(prefix)
        }
        .map { entry -> entry.displayName to DocumentsContract.buildDocumentUriUsingTree(treeUri, entry.documentId) }
    }
  }

  private fun copyBounded(input: InputStream, destination: File, maxBytes: Long) {
    input.use { source ->
      FileOutputStream(destination).use { target ->
        val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
        var total = 0L
        while (true) {
          val count = source.read(buffer)
          if (count < 0) break
          total += count
          require(total <= maxBytes) { "Selected document exceeds the size limit" }
          target.write(buffer, 0, count)
        }
        target.flush()
        target.fd.sync()
      }
    }
  }

  private fun validateDownloadMetadata(
    fileName: String,
    acceptedExtensions: List<String>,
    mimeType: String,
    documentKind: String,
  ) {
    require(fileName.isNotBlank()) { "Download file name is required" }
    require(fileName.length <= 255) { "Download file name is too long" }
    require('/' !in fileName && '\\' !in fileName) { "Download file name must not contain path separators" }
    require(acceptedExtensions.isNotEmpty()) { "At least one download extension is required" }
    require(DocumentTextCodec.hasAllowedExtension(fileName, acceptedExtensions)) {
      "$documentKind download uses an unsupported extension"
    }
    require(mimeType.isNotBlank() && '/' in mimeType) { "Download MIME type is invalid" }
  }

  private fun createPendingDownload(
    fileName: String,
    mimeType: String,
    relativePath: String = Environment.DIRECTORY_DOWNLOADS,
    collection: Uri = MediaStore.Downloads.EXTERNAL_CONTENT_URI,
  ): Uri {
    val pendingValues = ContentValues().apply {
      put(MediaStore.MediaColumns.DISPLAY_NAME, fileName)
      put(MediaStore.MediaColumns.MIME_TYPE, mimeType)
      put(MediaStore.MediaColumns.RELATIVE_PATH, relativePath)
      put(MediaStore.MediaColumns.IS_PENDING, 1)
    }
    return activity.contentResolver.insert(collection, pendingValues)
      ?: throw IllegalStateException("Android could not create the download")
  }

  private fun publishDownload(uri: Uri) {
    val published = ContentValues().apply { put(MediaStore.MediaColumns.IS_PENDING, 0) }
    check(activity.contentResolver.update(uri, published, null, null) == 1) {
      "Android could not publish the download"
    }
  }

  private fun resolveDisplayName(uri: Uri): String? {
    val projection = arrayOf(MediaStore.MediaColumns.DISPLAY_NAME)
    return activity.contentResolver.query(uri, projection, null, null, null)?.use { cursor ->
      if (!cursor.moveToFirst()) return@use null
      cursor.getString(0)
    }
  }

  @Suppress("DEPRECATION")
  private fun requireForeignPickerResult(uri: Uri) {
    val providerPackage = uri.authority?.let { authority ->
      activity.packageManager.resolveContentProvider(authority, 0)?.packageName
    }
    require(DocumentTextCodec.isAcceptedPickerResult(uri.scheme, uri.authority, providerPackage, activity.packageName)) {
      "The selected document is unavailable"
    }
  }

  private fun resolveOpenableDisplayName(uri: Uri): String? {
    val projection = arrayOf(OpenableColumns.DISPLAY_NAME)
    return activity.contentResolver.query(uri, projection, null, null, null)?.use { cursor ->
      if (!cursor.moveToFirst()) return@use null
      cursor.getString(0)
    }
  }
}
