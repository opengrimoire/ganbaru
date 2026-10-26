package org.opengrimoire.ganbaru.mobile.sync

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

  /** Encrypts [plaintext] bound to [aad] under the key of [alias], creating the key on first use. */
  fun wrap(alias: String, aad: ByteArray, plaintext: ByteArray): ByteArray = try {
    val cipher = Cipher.getInstance(TRANSFORMATION)
    cipher.init(Cipher.ENCRYPT_MODE, keyFor(alias))
    cipher.updateAAD(aad)
    val sealed = cipher.doFinal(plaintext)
    val nonce = cipher.iv
    if (nonce.size != NONCE_BYTES) {
      failure(STATUS_UNAVAILABLE, "UnexpectedNonceLength")
    } else {
      success(nonce + sealed)
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

  private fun keyStore(): KeyStore = KeyStore.getInstance(PROVIDER).apply { load(null) }

  private fun existingKey(alias: String): SecretKey? =
    keyStore().getKey(alias, null) as? SecretKey

  /** The key of [alias], generated once when missing. Generation is serialized per process. */
  @Synchronized
  private fun keyFor(alias: String): SecretKey = existingKey(alias) ?: generate(alias)

  private fun generate(alias: String): SecretKey = try {
    generate(alias, strongBox = true)
  } catch (error: StrongBoxUnavailableException) {
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

  private fun statusOf(error: GeneralSecurityException): Byte = when (error) {
    is AEADBadTagException,
    is KeyPermanentlyInvalidatedException,
    is UnrecoverableKeyException,
    -> STATUS_INVALIDATED
    else -> STATUS_UNAVAILABLE
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
