package org.enfein.mieru.service

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.VpnService
import android.os.Build
import androidx.core.app.NotificationCompat
import io.nekohasekai.libbox.InterfaceUpdateListener
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.enfein.mieru.R
import org.enfein.mieru.core.ConfigGenerator
import org.enfein.mieru.core.SanitizedLogger
import org.enfein.mieru.model.Profile
import org.enfein.mieru.model.VpnState
import org.enfein.mieru.model.VpnStatus
import org.enfein.mieru.ui.MainActivity
import java.net.HttpURLConnection
import java.net.URL

class MieruVpnService : VpnService() {

    private val serviceScope = CoroutineScope(Dispatchers.IO + SupervisorJob())
    private val commandMutex = Mutex()
    private var bridge: LibboxBridge? = null
    private var connectivityManager: ConnectivityManager? = null
    private var networkCallback: ConnectivityManager.NetworkCallback? = null
    private var watchdogJob: Job? = null

    companion object {
        const val ACTION_CONNECT = "org.enfein.mieru.ACTION_CONNECT"
        const val ACTION_DISCONNECT = "org.enfein.mieru.ACTION_DISCONNECT"

        const val EXTRA_SERVER = "extra_server"
        const val EXTRA_PORT = "extra_port"
        const val EXTRA_TRANSPORT = "extra_transport"
        const val EXTRA_USERNAME = "extra_username"
        const val EXTRA_PASSWORD = "extra_password"
        const val EXTRA_TRAFFIC_PATTERN = "extra_traffic_pattern"

        private const val NOTIFICATION_ID = 1001
        private const val CHANNEL_ID = "mieru_vpn_channel"

        private val _vpnStateFlow = MutableStateFlow(VpnState(VpnStatus.DISCONNECTED))
        val vpnStateFlow: StateFlow<VpnState> = _vpnStateFlow.asStateFlow()
    }

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        connectivityManager = getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val action = intent?.action ?: return START_NOT_STICKY

        serviceScope.launch {
            commandMutex.withLock {
                when (action) {
                    ACTION_CONNECT -> {
                        val serverExtra = intent.getStringExtra(EXTRA_SERVER)
                        val profile = if (!serverExtra.isNullOrEmpty()) {
                            val port = intent.getIntExtra(EXTRA_PORT, 8964)
                            val transport = intent.getStringExtra(EXTRA_TRANSPORT) ?: "TCP"
                            val username = intent.getStringExtra(EXTRA_USERNAME) ?: ""
                            val password = intent.getStringExtra(EXTRA_PASSWORD) ?: ""
                            val pattern = intent.getStringExtra(EXTRA_TRAFFIC_PATTERN)
                            Profile(serverExtra, port, transport, username, password, pattern)
                        } else {
                            val pendingFile = java.io.File(filesDir, "pending_profile.json")
                            if (pendingFile.exists()) {
                                val json = pendingFile.readText()
                                pendingFile.delete()
                                val p = Profile.fromJson(json).getOrNull()
                                if (p != null) {
                                    org.enfein.mieru.data.EncryptedProfileStorage(applicationContext).saveProfile(p)
                                    p
                                } else {
                                    org.enfein.mieru.data.EncryptedProfileStorage(applicationContext).loadProfile()
                                }
                            } else {
                                org.enfein.mieru.data.EncryptedProfileStorage(applicationContext).loadProfile()
                            }
                        }

                        if (profile == null || profile.validate().isFailure) {
                            SanitizedLogger.log("Cannot connect: no valid profile configured", "ERROR")
                            updateState(VpnStatus.ERROR, "No valid profile configured")
                            return@launch
                        }
                        handleConnect(profile)
                    }
                    ACTION_DISCONNECT -> {
                        handleDisconnect()
                    }
                }
            }
        }

        return START_NOT_STICKY
    }

    private suspend fun handleConnect(profile: Profile) {
        if (_vpnStateFlow.value.status == VpnStatus.CONNECTED || _vpnStateFlow.value.status == VpnStatus.CONNECTING) {
            SanitizedLogger.log("Already connecting or connected, ignoring connect command", "WARN")
            return
        }

        updateState(VpnStatus.CONNECTING, "Resolving server address…")
        startForeground(NOTIFICATION_ID, buildNotification("Connecting to ${profile.server}…"))

        try {
            SanitizedLogger.registerSecret(profile.password)
            SanitizedLogger.log("Connecting to ${profile.server}:${profile.serverPort} via ${profile.transport}")

            // 1. Resolve domain outside VPN
            val resolvedIp = ConfigGenerator.resolveServerAddress(profile.server)
            SanitizedLogger.log("Resolved server ${profile.server} -> $resolvedIp")

            // 2. Build core config
            val configJson = ConfigGenerator.generateCoreConfig(profile, resolvedIp)

            // 3. Initialize & start Libbox
            val libboxBridge = LibboxBridge(this)
            bridge = libboxBridge
            libboxBridge.initialize(applicationContext)

            updateState(VpnStatus.CONNECTING, "Starting core tunnel…")
            updateUnderlyingNetworks()
            libboxBridge.start(configJson)

            // 4. Register NetworkCallback for network changes
            registerNetworkCallback()
            updateUnderlyingNetworks()

            // 5. Bounded connectivity check through tunnel
            updateState(VpnStatus.CONNECTING, "Verifying tunnel connectivity…")
            val probeSuccess = verifyTunnelConnectivity()
            if (!probeSuccess) {
                throw IllegalStateException("Core tunnel started, but network probe through tunnel timed out or failed")
            }

            // 6. Connected!
            updateState(VpnStatus.CONNECTED, "Connected to ${profile.server}:${profile.serverPort}")
            updateNotification("Connected to ${profile.server}")
            SanitizedLogger.log("Tunnel verified successfully. Connected!")

            // Start watchdog to monitor tunnel health and never hide background failures
            startWatchdog()

        } catch (e: Exception) {
            val sanitizedMsg = SanitizedLogger.sanitize(e.message ?: "Connection failed")
            SanitizedLogger.log("Connection failed: $sanitizedMsg", "ERROR")
            handleCleanup()
            updateState(VpnStatus.ERROR, sanitizedMsg)
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
        }
    }

    private suspend fun verifyTunnelConnectivity(maxAttempts: Int = 4, delayBetweenMs: Long = 1000): Boolean = withContext(Dispatchers.IO) {
        val checkUrls = listOf(
            "http://connectivitycheck.gstatic.com/generate_204",
            "https://1.1.1.1",
            "https://www.google.com/generate_204"
        )
        for (attempt in 1..maxAttempts) {
            if (attempt > 1) {
                delay(delayBetweenMs)
            }
            for (urlString in checkUrls) {
                try {
                    SanitizedLogger.log("Performing connectivity probe to $urlString (attempt $attempt/$maxAttempts)…")
                    val url = URL(urlString)
                    val conn = url.openConnection() as HttpURLConnection
                    conn.connectTimeout = 4000
                    conn.readTimeout = 4000
                    conn.instanceFollowRedirects = false
                    val code = conn.responseCode
                    conn.disconnect()
                    if (code in 200..399 || code == 204) {
                        SanitizedLogger.log("Probe to $urlString returned HTTP $code")
                        return@withContext true
                    }
                } catch (e: Exception) {
                    SanitizedLogger.log("Probe to $urlString failed: ${e.message}", "WARN")
                }
            }
        }
        return@withContext false
    }

    private fun startWatchdog() {
        watchdogJob?.cancel()
        watchdogJob = serviceScope.launch {
            while (isActive) {
                delay(15000)
                if (_vpnStateFlow.value.status == VpnStatus.CONNECTED) {
                    val healthy = verifyTunnelConnectivity()
                    if (!healthy && _vpnStateFlow.value.status == VpnStatus.CONNECTED) {
                        val msg = "Connection lost: background tunnel probe failed"
                        SanitizedLogger.log(msg, "ERROR")
                        commandMutex.withLock {
                            handleCleanup()
                            updateState(VpnStatus.ERROR, msg)
                            stopForeground(STOP_FOREGROUND_REMOVE)
                            stopSelf()
                        }
                        break
                    }
                }
            }
        }
    }

    private fun handleDisconnect() {
        updateState(VpnStatus.DISCONNECTING, "Disconnecting…")
        updateNotification("Disconnecting…")
        SanitizedLogger.log("Disconnecting VPN…")

        handleCleanup()

        updateState(VpnStatus.DISCONNECTED, "")
        SanitizedLogger.log("Disconnected cleanly")
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun handleCleanup() {
        watchdogJob?.cancel()
        watchdogJob = null
        unregisterNetworkCallback()
        bridge?.stop()
        bridge = null
    }

    private fun registerNetworkCallback() {
        if (networkCallback != null) return
        val cm = connectivityManager ?: return
        val callback = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                SanitizedLogger.log("Network change: default network available")
                updateUnderlyingNetworks()
                bridge?.resetNetwork()
                updateDefaultInterfaceNotify()
            }
            override fun onLost(network: Network) {
                SanitizedLogger.log("Network change: default network lost")
                updateUnderlyingNetworks()
                bridge?.resetNetwork()
                updateDefaultInterfaceNotify()
            }
            override fun onCapabilitiesChanged(network: Network, networkCapabilities: NetworkCapabilities) {
                updateUnderlyingNetworks()
                updateDefaultInterfaceNotify()
            }
        }
        networkCallback = callback
        runCatching {
            cm.registerDefaultNetworkCallback(callback)
        }
    }

    private fun unregisterNetworkCallback() {
        networkCallback?.let { callback ->
            runCatching { connectivityManager?.unregisterNetworkCallback(callback) }
            networkCallback = null
        }
    }

    fun updateUnderlyingNetworks() {
        val cm = connectivityManager ?: return
        val networks = runCatching { cm.allNetworks }.getOrDefault(emptyArray())
        val physicalNetworks = networks.filter { network ->
            val caps = cm.getNetworkCapabilities(network) ?: return@filter false
            !caps.hasTransport(NetworkCapabilities.TRANSPORT_VPN) &&
                caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
        }
        if (physicalNetworks.isNotEmpty()) {
            setUnderlyingNetworks(physicalNetworks.toTypedArray())
            SanitizedLogger.log("Set underlying networks: ${physicalNetworks.size}")
        } else {
            setUnderlyingNetworks(null)
        }
    }

    private fun getPhysicalNetwork(): Network? {
        val cm = connectivityManager ?: return null
        val networks = runCatching { cm.allNetworks }.getOrDefault(emptyArray())
        for (network in networks) {
            val caps = cm.getNetworkCapabilities(network) ?: continue
            if (caps.hasTransport(NetworkCapabilities.TRANSPORT_VPN)) continue
            if (caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)) {
                return network
            }
        }
        return null
    }

    private fun updateDefaultInterfaceNotify() {
        val cm = connectivityManager ?: return
        val targetNetwork = getPhysicalNetwork() ?: return
        val lp = cm.getLinkProperties(targetNetwork) ?: return
        val caps = cm.getNetworkCapabilities(targetNetwork) ?: return

        val ifName = lp.interfaceName ?: ""
        if (ifName.isEmpty() || ifName.startsWith("tun")) return
        val ifIndex = runCatching { java.net.NetworkInterface.getByName(ifName)?.index }.getOrNull() ?: 0
        val isExpensive = !caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED)
        val isConstrained = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            !caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_CONGESTED)
        } else {
            isExpensive
        }
        bridge?.notifyDefaultInterface(ifName, ifIndex, isExpensive, isConstrained)
    }

    fun getDnsAndGatewaysForInterface(name: String): Pair<List<String>, List<String>> {
        val cm = connectivityManager ?: return Pair(emptyList(), emptyList())
        val dnsList = mutableListOf<String>()
        val gatewayList = mutableListOf<String>()
        val networks = runCatching { cm.allNetworks }.getOrDefault(emptyArray())
        for (network in networks) {
            val lp = cm.getLinkProperties(network) ?: continue
            if (lp.interfaceName == name) {
                for (dns in lp.dnsServers) {
                    dns.hostAddress?.let { dnsList.add(it) }
                }
                for (route in lp.routes) {
                    if (route.isDefaultRoute) {
                        route.gateway?.hostAddress?.let { gatewayList.add(it) }
                    }
                }
            }
        }
        return Pair(dnsList, gatewayList)
    }

    fun isNetworkMetered(name: String): Boolean {
        val cm = connectivityManager ?: return false
        val networks = runCatching { cm.allNetworks }.getOrDefault(emptyArray())
        for (network in networks) {
            val lp = cm.getLinkProperties(network) ?: continue
            if (lp.interfaceName == name) {
                val caps = cm.getNetworkCapabilities(network) ?: continue
                return !caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED)
            }
        }
        return false
    }

    fun reportCurrentDefaultInterface(listener: InterfaceUpdateListener) {
        val cm = connectivityManager ?: return
        val targetNetwork = getPhysicalNetwork() ?: return
        val lp = cm.getLinkProperties(targetNetwork) ?: return
        val caps = cm.getNetworkCapabilities(targetNetwork) ?: return

        val ifName = lp.interfaceName ?: ""
        if (ifName.isEmpty() || ifName.startsWith("tun")) return
        val ifIndex = runCatching { java.net.NetworkInterface.getByName(ifName)?.index }.getOrNull() ?: 0
        val isExpensive = !caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED)
        val isConstrained = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            !caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_CONGESTED)
        } else {
            isExpensive
        }
        runCatching {
            listener.updateDefaultInterface(ifName, ifIndex, isExpensive, isConstrained)
            listener.updateNetworkPath(ifName)
        }
    }

    override fun onRevoke() {
        SanitizedLogger.log("VPN revoked by system/user", "WARN")
        serviceScope.launch {
            commandMutex.withLock {
                handleDisconnect()
            }
        }
        super.onRevoke()
    }

    override fun onDestroy() {
        // Guarantee cleanup finishes synchronously before process termination
        runBlocking {
            commandMutex.withLock {
                handleCleanup()
                updateState(VpnStatus.DISCONNECTED, "")
            }
        }
        serviceScope.cancel()
        super.onDestroy()
    }

    private fun updateState(status: VpnStatus, message: String) {
        val sanitizedMessage = SanitizedLogger.sanitize(message)
        _vpnStateFlow.value = VpnState(status = status, message = sanitizedMessage)
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                CHANNEL_ID,
                "Mieru VPN Status",
                NotificationManager.IMPORTANCE_LOW
            ).apply {
                description = "Ongoing Mieru VPN connection status"
                setShowBadge(false)
            }
            val nm = getSystemService(NotificationManager::class.java)
            nm?.createNotificationChannel(channel)
        }
    }

    private fun buildNotification(statusText: String): Notification {
        val contentIntent = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        val disconnectIntent = PendingIntent.getService(
            this,
            1,
            Intent(this, MieruVpnService::class.java).apply { action = ACTION_DISCONNECT },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        val sanitizedStatus = SanitizedLogger.sanitize(statusText)

        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("Mieru VPN")
            .setContentText(sanitizedStatus)
            .setSmallIcon(R.drawable.ic_vpn_stat)
            .setContentIntent(contentIntent)
            .setOngoing(true)
            .addAction(
                NotificationCompat.Action.Builder(
                    0,
                    "Disconnect",
                    disconnectIntent
                ).build()
            )
            .build()
    }

    private fun updateNotification(statusText: String) {
        val nm = getSystemService(Context.NOTIFICATION_SERVICE) as? NotificationManager
        nm?.notify(NOTIFICATION_ID, buildNotification(statusText))
    }
}
