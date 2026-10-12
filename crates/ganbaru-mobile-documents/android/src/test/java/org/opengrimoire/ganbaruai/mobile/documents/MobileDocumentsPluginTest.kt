package org.opengrimoire.ganbaruai.mobile.documents

import java.io.ByteArrayInputStream
import java.nio.charset.MalformedInputException
import java.nio.file.Files
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class MobileDocumentsPluginTest {
  @Test
  fun acceptsOnlyConfiguredDocumentExtensions() {
    val allowed = listOf("json", ".m3u8", "M3U")

    assertEquals(true, DocumentTextCodec.hasAllowedExtension("focus.M3U8", allowed))
    assertEquals(true, DocumentTextCodec.hasAllowedExtension("library.json", allowed))
    assertEquals(false, DocumentTextCodec.hasAllowedExtension("library.json.txt", allowed))
    assertEquals(false, DocumentTextCodec.hasAllowedExtension("playlist", allowed))
  }

  @Test
  fun readsBoundedUtf8() {
    val input = ByteArrayInputStream("Ganbaru".toByteArray())

    assertEquals("Ganbaru", DocumentTextCodec.readUtf8(input, 7))
  }

  @Test
  fun rejectsDocumentsAboveTheLimit() {
    val input = ByteArrayInputStream("Ganbaru".toByteArray())

    assertThrows(IllegalArgumentException::class.java) {
      DocumentTextCodec.readUtf8(input, 6)
    }
  }

  @Test
  fun rejectsMalformedUtf8() {
    val input = ByteArrayInputStream(byteArrayOf(0xC3.toByte(), 0x28))

    assertThrows(MalformedInputException::class.java) {
      DocumentTextCodec.readUtf8(input, 2)
    }
  }

  @Test
  fun rejectsUnsafeVaultEntryNames() {
    assertEquals("notes", VaultTreePaths.validateDisplayName("notes"))
    for (name in listOf("", ".", "..", "notes/daily", "notes\\daily", "bad\u0000name")) {
      assertThrows(IllegalArgumentException::class.java) {
        VaultTreePaths.validateDisplayName(name)
      }
    }
  }

  @Test
  fun acceptsOnlyUnusedAppPrivateImportDestinations() {
    val dataRoot = Files.createTempDirectory("ganbaru-mobile-data").toFile()
    val outsideRoot = Files.createTempDirectory("ganbaru-mobile-outside").toFile()
    try {
      val destination = dataRoot.resolve("files/.Ganbaru AI.import")
      requireNotNull(destination.parentFile).mkdirs()
      assertEquals(
        destination.canonicalFile,
        VaultTreePaths.privateEmptyDestination(dataRoot, destination.path),
      )
      assertThrows(IllegalArgumentException::class.java) {
        VaultTreePaths.privateEmptyDestination(dataRoot, outsideRoot.resolve("vault").path)
      }
      destination.mkdir()
      assertThrows(IllegalArgumentException::class.java) {
        VaultTreePaths.privateEmptyDestination(dataRoot, destination.path)
      }
    } finally {
      dataRoot.deleteRecursively()
      outsideRoot.deleteRecursively()
    }
  }

  @Test
  fun validatesPrivateStreamingTransferPaths() {
    val dataRoot = Files.createTempDirectory("ganbaru-mobile-transfer-data").toFile()
    val outsideRoot = Files.createTempDirectory("ganbaru-mobile-transfer-outside").toFile()
    try {
      val transferRoot = dataRoot.resolve("cache/transfers")
      transferRoot.mkdirs()
      val destination = transferRoot.resolve("restore.ganbaru-backup")
      assertEquals(
        destination.canonicalFile,
        PrivateTransferPaths.unusedDestination(dataRoot, destination.path),
      )
      assertThrows(IllegalArgumentException::class.java) {
        PrivateTransferPaths.unusedDestination(dataRoot, outsideRoot.resolve("restore").path)
      }

      val source = transferRoot.resolve("backup.ganbaru-backup")
      source.writeBytes(byteArrayOf(1, 2, 3))
      assertEquals(
        source.canonicalFile,
        PrivateTransferPaths.readableSource(dataRoot, source.path),
      )
      assertThrows(IllegalArgumentException::class.java) {
        PrivateTransferPaths.readableSource(dataRoot, outsideRoot.resolve("backup").path)
      }
    } finally {
      dataRoot.deleteRecursively()
      outsideRoot.deleteRecursively()
    }
  }

  @Test
  fun enforcesVaultTreeCopyBudgets() {
    val entries = VaultTreeCopyBudget(VaultTreeCopyLimits(maxFiles = 2, maxBytes = 4, maxDepth = 1))
    entries.enter(0)
    entries.enter(1)
    entries.addBytes(4)
    assertThrows(IllegalArgumentException::class.java) { entries.enter(1) }

    val bytes = VaultTreeCopyBudget(VaultTreeCopyLimits(maxFiles = 2, maxBytes = 4, maxDepth = 1))
    assertThrows(IllegalArgumentException::class.java) { bytes.addBytes(5) }

    val depth = VaultTreeCopyBudget(VaultTreeCopyLimits(maxFiles = 2, maxBytes = 4, maxDepth = 1))
    assertThrows(IllegalArgumentException::class.java) { depth.enter(2) }
  }
}
