package org.enfein.mieru.model

enum class VpnStatus {
    DISCONNECTED,
    CONNECTING,
    CONNECTED,
    DISCONNECTING,
    ERROR
}

data class VpnState(
    val status: VpnStatus = VpnStatus.DISCONNECTED,
    val message: String = "",
    val since: Long = System.currentTimeMillis()
)
