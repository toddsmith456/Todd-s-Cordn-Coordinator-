package org.cordn.coordinator.service

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.AtomicFile
import android.util.Base64
import java.io.File
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import org.json.JSONObject

internal class IdentityStore(context: Context) {
    // A plain file, not SharedPreferences: the store is read from the
    // :coordinator process and written from the app process (reset), and
    // SharedPreferences caches per process — a stale cached ciphertext would
    // then be decrypted with a freshly generated Keystore key.
    private val file = AtomicFile(File(context.noBackupFilesDir, "cordn-identity.json"))

    fun getOrCreate(imported: String?): String {
        if (!imported.isNullOrBlank()) {
            val normalized = imported.trim().lowercase()
            require(normalized.matches(Regex("[0-9a-f]{64}"))) { "private key must be 64 hex characters" }
            store(normalized)
            return normalized
        }

        val stored = read()
        if (stored != null) {
            val (ciphertext, iv) = stored
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(
                Cipher.DECRYPT_MODE,
                key(),
                GCMParameterSpec(128, Base64.decode(iv, Base64.NO_WRAP)),
            )
            return String(cipher.doFinal(Base64.decode(ciphertext, Base64.NO_WRAP)), Charsets.UTF_8)
        }

        val secret = ByteArray(32).also(SecureRandom()::nextBytes).joinToString("") {
            (it.toInt() and 0xff).toString(16).padStart(2, '0')
        }
        store(secret)
        return secret
    }

    fun reset() {
        file.delete()
        val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        keyStore.deleteEntry(KEY_ALIAS)
    }

    private fun read(): Pair<String, String>? {
        if (!file.baseFile.exists()) return null
        return runCatching {
            val json = JSONObject(file.openRead().bufferedReader().use { it.readText() })
            json.getString("ciphertext") to json.getString("iv")
        }.getOrNull()
    }

    private fun store(secret: String) {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val json = JSONObject()
            .put(
                "ciphertext",
                Base64.encodeToString(cipher.doFinal(secret.toByteArray()), Base64.NO_WRAP),
            )
            .put("iv", Base64.encodeToString(cipher.iv, Base64.NO_WRAP))
        val output = file.startWrite()
        try {
            output.write(json.toString().toByteArray())
            file.finishWrite(output)
        } catch (error: Exception) {
            file.failWrite(output)
            throw error
        }
    }

    private fun key(): SecretKey {
        val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (keyStore.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }

        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").run {
            init(
                KeyGenParameterSpec.Builder(
                    KEY_ALIAS,
                    KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
                )
                    .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                    .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                    .build(),
            )
            generateKey()
        }
    }

    private companion object {
        const val KEY_ALIAS = "cordn-coordinator-identity"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
    }
}
