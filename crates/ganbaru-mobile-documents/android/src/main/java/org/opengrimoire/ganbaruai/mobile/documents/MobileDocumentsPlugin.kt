package org.opengrimoire.ganbaruai.mobile.documents

import android.app.Activity
import android.app.AlertDialog
import android.content.ComponentName
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

private fun primaryStorageRootUri(): Uri = DocumentsContract.buildDocumentUri(
  EXTERNAL_STORAGE_DOCUMENTS_AUTHORITY,
  PRIMARY_STORAGE_ROOT_DOCUMENT_ID,
)

private fun downloadsDirectoryUri(): Uri = DocumentsContract.buildDocumentUri(
  EXTERNAL_STORAGE_DOCUMENTS_AUTHORITY,
  "$PRIMARY_STORAGE_ROOT_DOCUMENT_ID${Environment.DIRECTORY_DOWNLOADS}",
)

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

private data class PendingDocumentCopy(
  val destination: File,
  val maxBytes: Long,
  val acceptedExtensions: List<String>,
  val documentKind: String,
)

internal object DocumentTextCodec {
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
  private var pendingReadLimit: Long? = null
  private var pendingReadExtensions: List<String> = emptyList()
  private var pendingReadDocumentKind = "document"
  private var pendingVaultTreeCopy: PendingVaultTreeCopy? = null
  private var pendingDocumentCopy: PendingDocumentCopy? = null

  @Command
  fun pickVaultTreeToPath(invoke: Invoke) {
    try {
      require(pendingVaultTreeCopy == null) { "A folder import is already active" }
      val args = invoke.parseArgs(PickVaultTreeToPathArgs::class.java)
      require(args.maxFiles > 0) { "Folder entry limit must be positive" }
      require(args.maxBytes > 0) { "Folder size limit must be positive" }
      require(args.maxDepth > 0) { "Folder depth limit must be positive" }
      val destination = VaultTreePaths.privateEmptyDestination(
        File(activity.applicationInfo.dataDir),
        args.destinationPath,
      )
      pendingVaultTreeCopy = PendingVaultTreeCopy(
        destination,
        VaultTreeCopyLimits(args.maxFiles, args.maxBytes, args.maxDepth),
      )
      val pickerIntent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        putExtra(DocumentsContract.EXTRA_INITIAL_URI, primaryStorageRootUri())
      }
      startActivityForResult(invoke, pickerIntent, "pickVaultTreeToPathResult")
    } catch (error: Exception) {
      pendingVaultTreeCopy = null
      invoke.reject(error.message ?: "Failed to open folder picker")
    }
  }

  @ActivityCallback
  fun pickVaultTreeToPathResult(invoke: Invoke, result: ActivityResult) {
    val pending = pendingVaultTreeCopy
    pendingVaultTreeCopy = null
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
      pendingReadLimit = args.maxBytes
      pendingReadExtensions = args.acceptedExtensions
      pendingReadDocumentKind = args.documentKind.trim()

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
            pendingReadLimit = null
            pendingReadExtensions = emptyList()
            pendingReadDocumentKind = "document"
            invoke.reject(error.message ?: "Failed to open document provider")
          }
          dialog.dismiss()
        }
        .setOnCancelListener {
          pendingReadLimit = null
          pendingReadExtensions = emptyList()
          pendingReadDocumentKind = "document"
          invoke.resolve(JSObject().apply { put("contents", null) })
        }
        .show()
    } catch (error: Exception) {
      pendingReadLimit = null
      pendingReadExtensions = emptyList()
      pendingReadDocumentKind = "document"
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
    val maxBytes = pendingReadLimit
    val acceptedExtensions = pendingReadExtensions
    val documentKind = pendingReadDocumentKind
    pendingReadLimit = null
    pendingReadExtensions = emptyList()
    pendingReadDocumentKind = "document"

    if (result.resultCode == Activity.RESULT_CANCELED) {
      invoke.resolve(JSObject().apply { put("contents", null) })
      return
    }
    if (result.resultCode != Activity.RESULT_OK || maxBytes == null) {
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
        val displayName = resolveOpenableDisplayName(uri)
          ?: throw IllegalArgumentException("The selected document has no file name")
        require(DocumentTextCodec.hasAllowedExtension(displayName, acceptedExtensions)) {
          "Select a supported $documentKind file"
        }
        val stream = activity.contentResolver.openInputStream(uri)
          ?: throw IllegalStateException("The selected document could not be opened")
        val contents = DocumentTextCodec.readUtf8(stream, maxBytes)
        invoke.resolve(JSObject().apply { put("contents", contents) })
      } catch (error: Exception) {
        invoke.reject(error.message ?: "Failed to read selected document")
      }
    }.start()
  }

  @Command
  fun pickDocumentToPath(invoke: Invoke) {
    try {
      require(pendingDocumentCopy == null) { "A document import is already active" }
      val args = invoke.parseArgs(PickDocumentToPathArgs::class.java)
      require(args.maxBytes > 0) { "Document size limit must be positive" }
      require(args.acceptedExtensions.isNotEmpty()) { "At least one document extension is required" }
      require(args.mimeTypes.isNotEmpty()) { "At least one document MIME type is required" }
      require(args.documentKind.isNotBlank()) { "Document kind is required" }
      val destination = PrivateTransferPaths.unusedDestination(
        File(activity.applicationInfo.dataDir),
        args.destinationPath,
      )
      pendingDocumentCopy = PendingDocumentCopy(
        destination,
        args.maxBytes,
        args.acceptedExtensions,
        args.documentKind.trim(),
      )
      val pickerIntent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
        addCategory(Intent.CATEGORY_OPENABLE)
        type = if (args.mimeTypes.size == 1) args.mimeTypes.single() else "*/*"
        if (args.mimeTypes.size > 1) putExtra(Intent.EXTRA_MIME_TYPES, args.mimeTypes.toTypedArray())
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        putExtra(DocumentsContract.EXTRA_INITIAL_URI, downloadsDirectoryUri())
      }
      startActivityForResult(invoke, pickerIntent, "pickDocumentToPathResult")
    } catch (error: Exception) {
      pendingDocumentCopy = null
      invoke.reject(error.message ?: "Failed to open document picker")
    }
  }

  @ActivityCallback
  fun pickDocumentToPathResult(invoke: Invoke, result: ActivityResult) {
    val pending = pendingDocumentCopy
    pendingDocumentCopy = null
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
        val displayName = resolveOpenableDisplayName(uri)
          ?: throw IllegalArgumentException("The selected document has no file name")
        require(DocumentTextCodec.hasAllowedExtension(displayName, pending.acceptedExtensions)) {
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

  private fun createPendingDownload(fileName: String, mimeType: String): Uri {
    val pendingValues = ContentValues().apply {
      put(MediaStore.MediaColumns.DISPLAY_NAME, fileName)
      put(MediaStore.MediaColumns.MIME_TYPE, mimeType)
      put(MediaStore.MediaColumns.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS)
      put(MediaStore.MediaColumns.IS_PENDING, 1)
    }
    return activity.contentResolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, pendingValues)
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

  private fun resolveOpenableDisplayName(uri: Uri): String? {
    val projection = arrayOf(OpenableColumns.DISPLAY_NAME)
    return activity.contentResolver.query(uri, projection, null, null, null)?.use { cursor ->
      if (!cursor.moveToFirst()) return@use null
      cursor.getString(0)
    }
  }
}
