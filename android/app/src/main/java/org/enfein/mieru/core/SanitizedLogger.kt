package org.enfein.mieru.core

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.enfein.mieru.model.LogEntry

object SanitizedLogger {
    private const val MAX_LOGS = 500
    private val logsList = mutableListOf<LogEntry>()
    private val _logsFlow = MutableStateFlow<List<LogEntry>>(emptyList())
    val logsFlow: StateFlow<List<LogEntry>> = _logsFlow.asStateFlow()

    private val activeSecrets = mutableSetOf<String>()

    @Synchronized
    fun registerSecret(secret: String) {
        if (secret.isNotEmpty()) {
            activeSecrets.add(secret)
        }
    }

    @Synchronized
    fun clearSecrets() {
        activeSecrets.clear()
    }

    @Synchronized
    fun sanitize(message: String): String {
        var sanitized = message
        for (secret in activeSecrets) {
            if (secret.isNotEmpty()) {
                sanitized = sanitized.replace(secret, "******")
            }
        }
        // General password sanitization regex
        sanitized = sanitized.replace(Regex("(?i)(\"password\"\\s*:\\s*\")[^\"]*(\")"), "$1******$2")
        sanitized = sanitized.replace(Regex("(?i)(password[=:][\\s]*)[^\\s,;]+"), "$1******")
        return sanitized
    }

    @Synchronized
    fun log(message: String, level: String = "INFO") {
        val sanitized = sanitize(message)
        val entry = LogEntry(level = level, message = sanitized)
        logsList.add(entry)
        if (logsList.size > MAX_LOGS) {
            logsList.removeAt(0)
        }
        _logsFlow.value = logsList.toList()
    }

    @Synchronized
    fun clear() {
        logsList.clear()
        _logsFlow.value = emptyList()
    }
}
