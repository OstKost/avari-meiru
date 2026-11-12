package org.enfein.mieru.ui

import android.app.Application
import android.content.Intent
import androidx.lifecycle.AndroidViewModel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.enfein.mieru.core.SanitizedLogger
import org.enfein.mieru.data.EncryptedProfileStorage
import org.enfein.mieru.model.LogEntry
import org.enfein.mieru.model.Profile
import org.enfein.mieru.model.VpnState
import org.enfein.mieru.model.VpnStatus
import org.enfein.mieru.service.MieruVpnService

class MainViewModel(application: Application) : AndroidViewModel(application) {

    private val storage = EncryptedProfileStorage(application)

    private val _profile = MutableStateFlow(
        storage.loadProfile() ?: Profile(
            server = "",
            serverPort = 8964,
            transport = "TCP",
            username = "",
            password = "",
            trafficPattern = null
        )
    )
    val profile: StateFlow<Profile> = _profile.asStateFlow()

    val vpnState: StateFlow<VpnState> = MieruVpnService.vpnStateFlow
    val logs: StateFlow<List<LogEntry>> = SanitizedLogger.logsFlow

    private val _errorMessage = MutableStateFlow<String?>(null)
    val errorMessage: StateFlow<String?> = _errorMessage.asStateFlow()

    init {
        val pending = java.io.File(application.filesDir, "pending_profile.json")
        if (pending.exists()) {
            val json = runCatching { pending.readText() }.getOrNull()
            pending.delete()
            if (!json.isNullOrBlank()) {
                Profile.fromJson(json).onSuccess {
                    updateProfile(it)
                }
            }
        }
    }

    fun updateProfile(newProfile: Profile) {
        _profile.value = newProfile
        storage.saveProfile(newProfile)
        SanitizedLogger.registerSecret(newProfile.password)
    }

    fun importProfileJson(json: String): Result<Unit> {
        return Profile.fromJson(json).map { imported ->
            updateProfile(imported)
            SanitizedLogger.log("Imported profile for server ${imported.server}:${imported.serverPort}")
            _errorMessage.value = null
        }.onFailure { ex ->
            _errorMessage.value = "Failed to import JSON: ${ex.message}"
            SanitizedLogger.log("Profile import failed: ${ex.message}", "WARN")
        }
    }

    fun exportProfileJson(): String {
        return _profile.value.toJson(includePassword = false)
    }

    fun clearError() {
        _errorMessage.value = null
    }

    fun clearLogs() {
        SanitizedLogger.clear()
    }

    fun connect() {
        val currentProfile = _profile.value
        val validation = currentProfile.validate()
        if (validation.isFailure) {
            val errorMsg = validation.exceptionOrNull()?.message ?: "Invalid profile"
            _errorMessage.value = errorMsg
            SanitizedLogger.log("Validation failed: $errorMsg", "ERROR")
            return
        }

        storage.saveProfile(currentProfile)
        SanitizedLogger.registerSecret(currentProfile.password)

        val app = getApplication<Application>()
        val intent = Intent(app, MieruVpnService::class.java).apply {
            action = MieruVpnService.ACTION_CONNECT
            putExtra(MieruVpnService.EXTRA_SERVER, currentProfile.server)
            putExtra(MieruVpnService.EXTRA_PORT, currentProfile.serverPort)
            putExtra(MieruVpnService.EXTRA_TRANSPORT, currentProfile.transport)
            putExtra(MieruVpnService.EXTRA_USERNAME, currentProfile.username)
            putExtra(MieruVpnService.EXTRA_PASSWORD, currentProfile.password)
            putExtra(MieruVpnService.EXTRA_TRAFFIC_PATTERN, currentProfile.trafficPattern)
        }
        app.startService(intent)
    }

    fun disconnect() {
        val app = getApplication<Application>()
        val intent = Intent(app, MieruVpnService::class.java).apply {
            action = MieruVpnService.ACTION_DISCONNECT
        }
        app.startService(intent)
    }
}
