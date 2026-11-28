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

internal class IdentityStore(
    context: Context,
    instanceDir: File,
    private val keyAlias: String = KEY_ALIAS_DEFAULT,
) {
    private val file = AtomicFile(File(instanceDir, "cordn-identity.json"))

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
                existingKey(),
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
        keyStore.deleteEntry(keyAlias)
    }

    private fun read(): Pair<String, String>? {
        // AtomicFile may have a recoverable backup even when the base is absent.
        val backup = File(file.baseFile.path + ".bak")
        if (!file.baseFile.exists() && !backup.exists()) return null
        return try {
            val json = JSONObject(file.openRead().bufferedReader().use { it.readText() })
            json.getString("ciphertext") to json.getString("iv")
        } catch (error: Exception) {
            throw IllegalStateException("Stored identity is unreadable; it was preserved. Do not reset without a backup.", error)
        }
    }

    private fun store(secret: String) {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val json = JSONObject()
            .put("ciphertext", Base64.encodeToString(cipher.doFinal(secret.toByteArray()), Base64.NO_WRAP))
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

    private fun existingKey(): SecretKey {
        val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        return keyStore.getKey(keyAlias, null) as? SecretKey
            ?: throw IllegalStateException("Stored identity key is unavailable; identity was preserved")
    }

    private fun key(): SecretKey {
        val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (keyStore.getKey(keyAlias, null) as? SecretKey)?.let { return it }
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").run {
            init(
                KeyGenParameterSpec.Builder(
                    keyAlias,
                    KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
                )
                    .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                    .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                    .build(),
            )
            generateKey()
        }
    }

    companion object {
        const val KEY_ALIAS_DEFAULT = "cordn-coordinator-identity"
        private const val TRANSFORMATION = "AES/GCM/NoPadding"
        fun aliasFor(instanceId: String): String = if (instanceId == "default") KEY_ALIAS_DEFAULT else "cordn-coordinator-identity-$instanceId"
    }
}
