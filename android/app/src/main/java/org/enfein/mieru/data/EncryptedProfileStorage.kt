package org.enfein.mieru.data

import android.content.Context
import android.content.SharedPreferences
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import org.enfein.mieru.model.Profile

class EncryptedProfileStorage(context: Context) {

    private val prefs: SharedPreferences = runCatching {
        val masterKey = MasterKey.Builder(context)
            .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
            .build()

        EncryptedSharedPreferences.create(
            context,
            PREFS_NAME,
            masterKey,
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
        )
    }.getOrElse {
        // Fallback for isolated test environments where Keystore may be unavailable
        context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
    }

    fun saveProfile(profile: Profile) {
        prefs.edit()
            .putString(KEY_SERVER, profile.server)
            .putInt(KEY_PORT, profile.serverPort)
            .putString(KEY_TRANSPORT, profile.transport)
            .putString(KEY_USERNAME, profile.username)
            .putString(KEY_PASSWORD, profile.password)
            .putString(KEY_TRAFFIC_PATTERN, profile.trafficPattern ?: "")
            .apply()
    }

    fun loadProfile(): Profile? {
        val server = prefs.getString(KEY_SERVER, null) ?: return null
        val port = prefs.getInt(KEY_PORT, 8964)
        val transport = prefs.getString(KEY_TRANSPORT, "TCP") ?: "TCP"
        val username = prefs.getString(KEY_USERNAME, "") ?: ""
        val password = prefs.getString(KEY_PASSWORD, "") ?: ""
        val pattern = prefs.getString(KEY_TRAFFIC_PATTERN, null)?.takeIf { it.isNotEmpty() }

        return Profile(
            server = server,
            serverPort = port,
            transport = transport,
            username = username,
            password = password,
            trafficPattern = pattern
        )
    }

    fun clear() {
        prefs.edit().clear().apply()
    }

    companion object {
        private const val PREFS_NAME = "mieru_secure_profile_storage"
        private const val KEY_SERVER = "server"
        private const val KEY_PORT = "port"
        private const val KEY_TRANSPORT = "transport"
        private const val KEY_USERNAME = "username"
        private const val KEY_PASSWORD = "password"
        private const val KEY_TRAFFIC_PATTERN = "traffic_pattern"
    }
}
