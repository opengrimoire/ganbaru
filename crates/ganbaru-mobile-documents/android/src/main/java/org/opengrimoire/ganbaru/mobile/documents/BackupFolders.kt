package org.opengrimoire.ganbaru.mobile.documents

import app.tauri.annotation.InvokeArg

/** Longest file name prefix an automatic backup folder lists or deletes by. */
private const val MAX_BACKUP_PREFIX_CHARS = 128

/**
 * Requests on the folder automatic backups go to: a folder below Documents that the app writes
 * through MediaStore, or a document tree the user picked and granted persistent access to.
 * Exactly one of `treeUri` and `documentsSubdirectory` is set.
 */
@InvokeArg
internal class BackupFolderFilesArgs {
  var treeUri: String? = null
  var documentsSubdirectory: String? = null
  var prefix: String = ""
  var names: List<String> = emptyList()
}

@InvokeArg
internal class SaveFileToBackupFolderArgs {
  var treeUri: String? = null
  var documentsSubdirectory: String? = null
  lateinit var sourcePath: String
  lateinit var fileName: String
  var maxBytes: Long = 0
  var acceptedExtensions: List<String> = emptyList()
  var mimeType: String = "application/octet-stream"
  var documentKind: String = "document"
}

@InvokeArg
internal class BackupTreeArgs {
  var treeUri: String = ""
}

/** A validated automatic backup folder. */
internal sealed interface BackupFolder {
  /** A folder below Documents, as the MediaStore relative path of its entries. */
  data class Documents(val relativePath: String) : BackupFolder

  /** A document tree the user picked, as its tree URI text. */
  data class Tree(val uri: String) : BackupFolder
}

internal object BackupFolders {
  /**
   * The folder [treeUri] or [documentsSubdirectory] names, where Documents lives at
   * [documentsDirectory]. Exactly one must be set, and a Documents folder is one plain folder name.
   */
  fun parse(treeUri: String?, documentsSubdirectory: String?, documentsDirectory: String): BackupFolder {
    require((treeUri == null) != (documentsSubdirectory == null)) {
      "Exactly one backup folder must be named"
    }
    if (treeUri != null) {
      require(treeUri.startsWith("content://")) { "The backup folder is unavailable" }
      return BackupFolder.Tree(treeUri)
    }
    val name = VaultTreePaths.validateDisplayName(documentsSubdirectory.orEmpty())
    require(name.length <= 255 && name.trim() == name) { "The backup folder name is invalid" }
    return BackupFolder.Documents("$documentsDirectory/$name/")
  }

  /** Validates the file name prefix that a list or delete is limited to. */
  fun validatePrefix(prefix: String): String {
    require(prefix.isNotBlank() && prefix.length <= MAX_BACKUP_PREFIX_CHARS) {
      "Backup file prefix is invalid"
    }
    require('/' !in prefix && '\\' !in prefix && '\u0000' !in prefix) {
      "Backup file prefix is invalid"
    }
    return prefix
  }

  /** A SQL `LIKE` pattern, escaped with a backslash, that matches names starting with [prefix]. */
  fun likePattern(prefix: String): String =
    prefix.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_") + "%"

  /** The names among [present] that start with [prefix] and were asked for in [requested]. */
  fun deletable(present: List<String>, prefix: String, requested: List<String>): List<String> {
    val wanted = requested.toSet()
    return present.filter { name -> name.startsWith(prefix) && name in wanted }
  }
}
