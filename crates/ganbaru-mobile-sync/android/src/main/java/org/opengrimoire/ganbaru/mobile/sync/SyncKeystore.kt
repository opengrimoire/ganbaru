package org.opengrimoire.ganbaru.mobile.sync

import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import android.security.keystore.StrongBoxUnavailableException
import java.io.IOException
import java.security.GeneralSecurityException
import java.security.KeyStore
import java.security.ProviderException
import java.security.UnrecoverableKeyException
import javax.crypto.AEADBadTagException
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Seals the device-local key files Rust keeps with AES-256-GCM keys held by the Android Keystore.
 * Keys need no user authentication, so a background pass can use them, and prefer StrongBox when
 * the device has it.
 *
 * Rust calls these methods through JNI after [SyncNative] registers the object. Every result is a
 * byte array whose first byte is a status: [STATUS_OK] followed by the payload, or a failure status
 * followed by the exception class name. The bridge never throws, and it zeroes the secret inputs it
 * receives before returning.
 */
internal object SyncKeystore {
  /** The result carries the payload. */
  private const val STATUS_OK: Byte = 0

  /** The platform key cannot be used now; a later attempt may succeed. */
  private const val STATUS_UNAVAILABLE: Byte = 1

  /** The platform key is gone or no longer opens what it sealed. */
  private const val STATUS_INVALIDATED: Byte = 2

  private const val PROVIDER = "AndroidKeyStore"
  private const val TRANSFORMATION = "AES/GCM/NoPadding"
  private const val KEY_BITS = 256
  private const val TAG_BITS = 128
  private const val NONCE_BYTES = 12
  private const val MAX_DETAIL_BYTES = 256
  private const val MAX_CAUSE_DEPTH = 8

  /** Keystore error codes (API 33+) that mean the key can never be used again. */
  private val PERMANENT_KEYSTORE_ERRORS = setOf(
    android.security.KeyStoreException.ERROR_KEY_DOES_NOT_EXIST,
    android.security.KeyStoreException.ERROR_KEY_CORRUPTED,
  )

  /**
   * Encrypts [plaintext] bound to [aad] under the key of [alias], creating the key on first use. A
   * key that was permanently invalidated is replaced, since nothing it sealed opens again anyway.
   */
  fun wrap(alias: String, aad: ByteArray, plaintext: ByteArray): ByteArray = try {
    try {
      seal(alias, aad, plaintext)
    } catch (error: KeyPermanentlyInvalidatedException) {
      keyStore().deleteEntry(alias)
      seal(alias, aad, plaintext)
    }
  } catch (error: GeneralSecurityException) {
    failure(statusOf(error), error)
  } catch (error: ProviderException) {
    failure(STATUS_UNAVAILABLE, error)
  } catch (error: IOException) {
    failure(STATUS_UNAVAILABLE, error)
  } finally {
    plaintext.fill(0)
  }

  /** Decrypts a nonce and ciphertext [wrap] returned for [alias] and [aad]. */
  fun unwrap(alias: String, aad: ByteArray, sealed: ByteArray): ByteArray {
    if (sealed.size <= NONCE_BYTES) return failure(STATUS_INVALIDATED, "ShortCiphertext")
    return try {
      val key = existingKey(alias) ?: return failure(STATUS_INVALIDATED, "MissingKey")
      val cipher = Cipher.getInstance(TRANSFORMATION)
      cipher.init(
        Cipher.DECRYPT_MODE,
        key,
        GCMParameterSpec(TAG_BITS, sealed, 0, NONCE_BYTES),
      )
      cipher.updateAAD(aad)
      val plaintext = cipher.doFinal(sealed, NONCE_BYTES, sealed.size - NONCE_BYTES)
      try {
        success(plaintext)
      } finally {
        plaintext.fill(0)
      }
    } catch (error: GeneralSecurityException) {
      failure(statusOf(error), error)
    } catch (error: ProviderException) {
      failure(STATUS_UNAVAILABLE, error)
    } catch (error: IOException) {
      failure(STATUS_UNAVAILABLE, error)
    }
  }

  /** Deletes the key of [alias]; a missing key is already forgotten. */
  fun forget(alias: String): ByteArray = try {
    keyStore().deleteEntry(alias)
    success(ByteArray(0))
  } catch (error: GeneralSecurityException) {
    failure(STATUS_UNAVAILABLE, error)
  } catch (error: IOException) {
    failure(STATUS_UNAVAILABLE, error)
  }

  private fun seal(alias: String, aad: ByteArray, plaintext: ByteArray): ByteArray {
    val cipher = Cipher.getInstance(TRANSFORMATION)
    cipher.init(Cipher.ENCRYPT_MODE, keyFor(alias))
    cipher.updateAAD(aad)
    val sealed = cipher.doFinal(plaintext)
    val nonce = cipher.iv
    return if (nonce.size != NONCE_BYTES) {
      failure(STATUS_UNAVAILABLE, "UnexpectedNonceLength")
    } else {
      success(nonce + sealed)
    }
  }

  private fun keyStore(): KeyStore = KeyStore.getInstance(PROVIDER).apply { load(null) }

  private fun existingKey(alias: String): SecretKey? =
    keyStore().getKey(alias, null) as? SecretKey

  /**
   * The key of [alias], generated once when missing or permanently lost. Generation is serialized
   * per process.
   */
  @Synchronized
  private fun keyFor(alias: String): SecretKey {
    val existing = try {
      existingKey(alias)
    } catch (error: UnrecoverableKeyException) {
      if (!isPermanentLoss(error)) throw error
      keyStore().deleteEntry(alias)
      null
    }
    return existing ?: generate(alias)
  }

  /**
   * Generates the key of [alias] in StrongBox, or in the regular Keystore when StrongBox refuses.
   * StrongBox reports more than [StrongBoxUnavailableException]: some devices fail generation
   * with a plain [ProviderException], so any provider failure falls back.
   */
  private fun generate(alias: String): SecretKey = try {
    generate(alias, strongBox = true)
  } catch (error: ProviderException) {
    generate(alias, strongBox = false)
  }

  private fun generate(alias: String, strongBox: Boolean): SecretKey {
    val spec = KeyGenParameterSpec.Builder(
      alias,
      KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
    )
      .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
      .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
      .setKeySize(KEY_BITS)
      .setRandomizedEncryptionRequired(true)
      .setIsStrongBoxBacked(strongBox)
      .build()
    val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, PROVIDER)
    generator.init(spec)
    return generator.generateKey()
  }

  /**
   * Whether [error] means the key is gone for good. [UnrecoverableKeyException] also wraps
   * transient Keystore failures, such as a busy or restarting keystore daemon, so it counts only
   * when its cause chain shows permanent invalidation or a key that no longer exists.
   */
  private fun statusOf(error: GeneralSecurityException): Byte = when (error) {
    is AEADBadTagException,
    is KeyPermanentlyInvalidatedException,
    -> STATUS_INVALIDATED
    is UnrecoverableKeyException ->
      if (isPermanentLoss(error)) STATUS_INVALIDATED else STATUS_UNAVAILABLE
    else -> STATUS_UNAVAILABLE
  }

  private fun isPermanentLoss(error: Throwable): Boolean {
    var cause: Throwable? = error
    var depth = 0
    while (cause != null && depth < MAX_CAUSE_DEPTH) {
      if (cause is KeyPermanentlyInvalidatedException) return true
      if (
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
        cause is android.security.KeyStoreException &&
        cause.numericErrorCode in PERMANENT_KEYSTORE_ERRORS
      ) {
        return true
      }
      cause = cause.cause
      depth += 1
    }
    return false
  }

  private fun success(payload: ByteArray): ByteArray {
    val result = ByteArray(payload.size + 1)
    result[0] = STATUS_OK
    payload.copyInto(result, destinationOffset = 1)
    return result
  }

  private fun failure(status: Byte, error: Throwable): ByteArray =
    failure(status, error.javaClass.simpleName)

  private fun failure(status: Byte, detail: String): ByteArray {
    val bytes = detail.toByteArray(Charsets.UTF_8)
    val length = minOf(bytes.size, MAX_DETAIL_BYTES)
    val result = ByteArray(length + 1)
    result[0] = status
    bytes.copyInto(result, destinationOffset = 1, endIndex = length)
    return result
  }
}
