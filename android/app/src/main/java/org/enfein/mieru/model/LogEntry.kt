package org.enfein.mieru.model

import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

data class LogEntry(
    val timestamp: Long = System.currentTimeMillis(),
    val level: String = "INFO",
    val message: String
) {
    fun format(): String {
        val sdf = SimpleDateFormat("HH:mm:ss.SSS", Locale.US)
        return "[${sdf.format(Date(timestamp))}] [$level] $message"
    }
}
