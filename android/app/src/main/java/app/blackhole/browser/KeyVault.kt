package app.blackhole.browser

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * The key that seals the paired devices' keys (`devices.bin`), the role the system keychain
 * plays on Windows and macOS. A random 32-byte key is kept encrypted (AES-GCM) under a key of
 * the Android Keystore that never leaves it; only the wrapped form is stored, in private
 * preferences excluded from backups.
 */
object KeyVault {
    private const val ALIAS = "blackhole-vault-wrap"
    private const val PREFS = "vault"
    private const val ENTRY = "devices-key"

    private fun wrappingKey(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getEntry(ALIAS, null) as? KeyStore.SecretKeyEntry)?.let { return it.secretKey }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return generator.generateKey()
    }

    /** The key as 64 hex digits; created on first use. Empty when the Keystore fails. */
    fun devicesKeyHex(context: Context): String = try {
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val raw = prefs.getString(ENTRY, null)?.let { stored ->
            val bytes = Base64.decode(stored, Base64.NO_WRAP)
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, wrappingKey(), GCMParameterSpec(128, bytes, 0, 12))
            cipher.doFinal(bytes, 12, bytes.size - 12)
        } ?: ByteArray(32).also { key ->
            SecureRandom().nextBytes(key)
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.ENCRYPT_MODE, wrappingKey())
            val sealed = cipher.iv + cipher.doFinal(key)
            prefs.edit().putString(ENTRY, Base64.encodeToString(sealed, Base64.NO_WRAP)).apply()
        }
        val hex = raw.joinToString("") { "%02x".format(it) }
        raw.fill(0)
        hex
    } catch (e: java.security.GeneralSecurityException) {
        ""
    } catch (e: java.io.IOException) {
        ""
    }
}
