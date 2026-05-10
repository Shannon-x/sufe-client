package com.xboard.client.secure

import android.content.Context
import android.content.SharedPreferences
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import com.xboard.client.core.SecureStore
import com.xboard.client.core.StorageException

/**
 * Implements the UniFFI `SecureStore` callback interface using
 * [EncryptedSharedPreferences] backed by an Android Keystore master key.
 *
 * Why EncryptedSharedPreferences over the lower-level `KeyStore` API:
 *
 *   - Per-key entries (we store the bearer + LoginSummary blob + subscribe
 *     URL separately) map cleanly to SharedPreferences semantics.
 *   - The library handles key rotation + IV management; the Rust side
 *     only sees opaque strings.
 *   - It's the AndroidX-recommended approach for "small string secrets,"
 *     which is exactly our use case.
 *
 * NOTE: `MasterKey.Builder` will provision the keystore entry on first
 * use. That involves a Keystore + JCE round trip and can take 30-100ms,
 * so callers should resolve [INSTANCE] off the main thread (we do, via
 * [AppContainer]'s `applicationScope` lazy init).
 */
class AndroidSecureStore(context: Context) : SecureStore {

    private val prefs: SharedPreferences = run {
        val masterKey = MasterKey.Builder(context.applicationContext)
            .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
            .build()

        EncryptedSharedPreferences.create(
            context.applicationContext,
            PREFS_NAME,
            masterKey,
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM,
        )
    }

    override fun put(key: String, value: String) {
        runCatching {
            prefs.edit().putString(key, value).apply()
        }.onFailure {
            throw StorageException.Backend(it.message ?: "EncryptedSharedPreferences.put failed")
        }
    }

    override fun get(key: String): String? = runCatching {
        prefs.getString(key, null)
    }.getOrElse {
        throw StorageException.Backend(it.message ?: "EncryptedSharedPreferences.get failed")
    }

    override fun delete(key: String) {
        runCatching {
            prefs.edit().remove(key).apply()
        }.onFailure {
            throw StorageException.Backend(it.message ?: "EncryptedSharedPreferences.delete failed")
        }
    }

    companion object {
        // Backup rules (xml/backup_rules.xml) reference this name —
        // keep them in sync if you rename.
        private const val PREFS_NAME = "xboard_secure_prefs"
    }
}
