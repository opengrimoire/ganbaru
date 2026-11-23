package org.opengrimoire.ganbaru.mobile.documents

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class BackupFoldersTest {
  @Test
  fun namesOneDocumentsFolderOrOnePickedTree() {
    assertEquals(
      BackupFolder.Documents("Documents/Ganbaru Backup/"),
      BackupFolders.parse(null, "Ganbaru Backup", "Documents"),
    )
    assertEquals(
      BackupFolder.Tree("content://com.android.externalstorage.documents/tree/primary%3ABackups"),
      BackupFolders.parse("content://com.android.externalstorage.documents/tree/primary%3ABackups", null, "Documents"),
    )
    for ((tree, documents) in listOf(null to null, "content://tree" to "Backups", "file:///sdcard" to null)) {
      assertThrows(IllegalArgumentException::class.java) {
        BackupFolders.parse(tree, documents, "Documents")
      }
    }
  }

  @Test
  fun refusesDocumentsFoldersThatAreNotOnePlainName() {
    for (name in listOf("", "..", "a/b", "a\\b", " padded ")) {
      assertThrows(IllegalArgumentException::class.java) {
        BackupFolders.parse(null, name, "Documents")
      }
    }
  }

  @Test
  fun escapesLikePatternWildcards() {
    assertEquals("ganbaru\\_auto-1\\%\\\\%", BackupFolders.likePattern("ganbaru_auto-1%\\"))
  }

  @Test
  fun refusesPrefixesThatCouldNameOtherFiles() {
    assertEquals("ganbaru-auto-", BackupFolders.validatePrefix("ganbaru-auto-"))
    for (prefix in listOf("", " ", "a/b", "x".repeat(129))) {
      assertThrows(IllegalArgumentException::class.java) {
        BackupFolders.validatePrefix(prefix)
      }
    }
  }

  @Test
  fun deletesOnlyRequestedFilesWithThePrefix() {
    val present = listOf("auto-1.ganbaru-backup", "auto-2.ganbaru-backup", "manual.ganbaru-backup")

    assertEquals(
      listOf("auto-1.ganbaru-backup"),
      BackupFolders.deletable(present, "auto-", listOf("auto-1.ganbaru-backup", "manual.ganbaru-backup")),
    )
  }
}
