package org.enfein.mieru.service

import android.os.ParcelFileDescriptor
import io.nekohasekai.libbox.*
import org.enfein.mieru.core.SanitizedLogger
import java.io.File
import java.net.Inet4Address
import java.net.NetworkInterface
import java.util.Collections
import java.util.concurrent.CopyOnWriteArrayList

class SimpleStringIterator(private val list: List<String>) : StringIterator {
    private var index = 0
    override fun hasNext(): Boolean = index < list.size
    override fun len(): Int = list.size
    override fun next(): String = if (hasNext()) list[index++] else ""
}

class LibboxBridge(
    private val vpnService: MieruVpnService
) : PlatformInterface, CommandServerHandler {

    private var commandServer: CommandServer? = null
    private var activePfd: ParcelFileDescriptor? = null
    private val interfaceListeners = CopyOnWriteArrayList<InterfaceUpdateListener>()

    fun initialize(context: android.content.Context) {
        val setupOptions = SetupOptions().apply {
            basePath = context.filesDir.absolutePath
            workingPath = File(context.filesDir, "working").apply { mkdirs() }.absolutePath
            tempPath = File(context.cacheDir, "temp").apply { mkdirs() }.absolutePath
            fixAndroidStack = true
            commandServerListenPort = 0
            commandServerSecret = ""
            logMaxLines = 500L
            debug = false
            crashReportSource = "Android"
            appVersion = "1.0.0"
            appMarketingVersion = "1.0.0"
            oomKillerEnabled = false
            oomKillerDisabled = true
            oomMemoryLimit = 0L
            powerReportEnabled = false
            platformMetadata = ""
        }
        Libbox.setup(setupOptions)
        SanitizedLogger.log("Libbox initialized version ${Libbox.version()}")
    }

    fun start(configJson: String) {
        val server = Libbox.newCommandServer(this, this)
        commandServer = server
        server.start()
        SanitizedLogger.log("Libbox CommandServer started")
        server.startOrReloadService(configJson, OverrideOptions())
        SanitizedLogger.log("Libbox Service started")

        // Upstream duplicates fd inside OpenInterface, so close our local ParcelFileDescriptor safely
        activePfd?.let { pfd ->
            runCatching { pfd.close() }
            activePfd = null
            SanitizedLogger.log("Original ParcelFileDescriptor closed (upstream duplicated fd)")
        }
    }

    fun stop() {
        val server = commandServer
        commandServer = null

        if (server != null) {
            runCatching {
                server.closeService()
            }.onFailure {
                SanitizedLogger.log("closeService failed: ${it.message}", "WARN")
            }

            runCatching {
                server.close()
            }.onFailure {
                SanitizedLogger.log("commandServer.close failed: ${it.message}", "WARN")
            }
        }

        activePfd?.let { pfd ->
            activePfd = null
            runCatching { pfd.close() }.onFailure {
                SanitizedLogger.log("pfd.close failed: ${it.message}", "WARN")
            }
        }

        SanitizedLogger.log("Libbox stopped cleanly")
    }

    fun resetNetwork() {
        runCatching {
            commandServer?.resetNetwork()
            SanitizedLogger.log("Libbox network reset triggered")
        }
    }

    fun notifyDefaultInterface(
        interfaceName: String,
        interfaceIndex: Int,
        isExpensive: Boolean,
        isConstrained: Boolean
    ) {
        for (listener in interfaceListeners) {
            runCatching {
                listener.updateDefaultInterface(interfaceName, interfaceIndex, isExpensive, isConstrained)
                listener.updateNetworkPath(interfaceName)
            }
        }
    }

    // PlatformInterface implementation
    override fun usePlatformAutoDetectInterfaceControl(): Boolean = true

    override fun autoDetectInterfaceControl(fd: Int) {
        val protected = vpnService.protect(fd)
        if (!protected) {
            val msg = "VpnService.protect failed for socket fd $fd"
            SanitizedLogger.log(msg, "ERROR")
            throw IllegalStateException(msg)
        }
    }

    override fun openTun(options: TunOptions): Int {
        val builder = vpnService.Builder()
        builder.setSession("Mieru VPN")

        val mtu = options.mtu
        builder.setMtu(if (mtu > 0) mtu else 1500)

        // Addresses
        val inet4Iterator = options.inet4Address
        var hasV4 = false
        while (inet4Iterator != null && inet4Iterator.hasNext()) {
            val prefix = inet4Iterator.next()
            if (prefix != null) {
                builder.addAddress(prefix.address(), prefix.prefix())
                hasV4 = true
            }
        }
        if (!hasV4) {
            builder.addAddress("172.19.0.1", 30)
        }

        val inet6Iterator = options.inet6Address
        var hasV6 = false
        while (inet6Iterator != null && inet6Iterator.hasNext()) {
            val prefix = inet6Iterator.next()
            if (prefix != null) {
                builder.addAddress(prefix.address(), prefix.prefix())
                hasV6 = true
            }
        }
        if (!hasV6) {
            builder.addAddress("fdfe:dcba:9876::1", 126)
        }

        // Full IPv4 tunnel: 0.0.0.0/0
        builder.addRoute("0.0.0.0", 0)

        // Full IPv6 tunnel: ::/0 to prevent IPv6 bypass
        builder.addRoute("::", 0)

        // DNS Servers
        val dnsIterator = runCatching { options.dnsServerAddress }.getOrNull()
        var hasDns = false
        while (dnsIterator != null && dnsIterator.hasNext()) {
            val dns = dnsIterator.next()
            if (!dns.isNullOrEmpty()) {
                builder.addDnsServer(dns)
                hasDns = true
            }
        }
        if (!hasDns) {
            builder.addDnsServer("1.1.1.1")
        }

        val pfd = builder.establish() ?: throw IllegalStateException("Failed to establish VPN tunnel (builder.establish() returned null)")
        activePfd = pfd
        val fd = pfd.fd
        SanitizedLogger.log("VPN TUN established fd=$fd, MTU=${if (mtu > 0) mtu else 1500}, full IPv4 & IPv6 routed")
        return fd
    }

    override fun useProcFS(): Boolean = false

    override fun findConnectionOwner(
        ipProtocol: Int,
        sourceAddress: String,
        sourcePort: Int,
        destinationAddress: String,
        destinationPort: Int
    ): ConnectionOwner {
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.Q) {
            val cm = vpnService.getSystemService(android.content.Context.CONNECTIVITY_SERVICE) as? android.net.ConnectivityManager
            if (cm != null) {
                try {
                    val src = java.net.InetSocketAddress(java.net.InetAddress.getByName(sourceAddress), sourcePort)
                    val dst = java.net.InetSocketAddress(java.net.InetAddress.getByName(destinationAddress), destinationPort)
                    val uid = cm.getConnectionOwnerUid(ipProtocol, src, dst)
                    if (uid != android.os.Process.INVALID_UID && uid > 0) {
                        val owner = ConnectionOwner()
                        owner.userId = uid
                        val packageNames = vpnService.packageManager.getPackagesForUid(uid)
                        if (packageNames != null && packageNames.isNotEmpty()) {
                            owner.setAndroidPackageNames(SimpleStringIterator(packageNames.toList()))
                        }
                        return owner
                    }
                } catch (e: Exception) {
                    // Fall through to throw
                }
            }
        }
        throw Exception("Connection owner not found")
    }

    override fun startDefaultInterfaceMonitor(listener: InterfaceUpdateListener) {
        interfaceListeners.add(listener)
        vpnService.reportCurrentDefaultInterface(listener)
    }

    override fun closeDefaultInterfaceMonitor(listener: InterfaceUpdateListener) {
        interfaceListeners.remove(listener)
    }

    override fun getInterfaces(): NetworkInterfaceIterator {
        val resultList = mutableListOf<io.nekohasekai.libbox.NetworkInterface>()
        val nifs = runCatching { Collections.list(NetworkInterface.getNetworkInterfaces()) }.getOrDefault(emptyList())

        for (nif in nifs) {
            if (nif.name.startsWith("tun")) continue
            val netInterface = io.nekohasekai.libbox.NetworkInterface()
            netInterface.name = nif.name
            netInterface.index = nif.index
            netInterface.mtu = runCatching { nif.mtu }.getOrDefault(1500)

            // Flags
            var flags = 0
            if (runCatching { nif.isUp }.getOrDefault(false)) flags = flags or 1
            if (runCatching { nif.isLoopback }.getOrDefault(false)) flags = flags or 8
            if (runCatching { nif.isPointToPoint }.getOrDefault(false)) flags = flags or 16
            if (runCatching { nif.supportsMulticast() }.getOrDefault(false)) flags = flags or 1024
            netInterface.flags = flags

            // Type
            val type = when {
                nif.name.startsWith("wlan") || nif.name.startsWith("wifi") -> Libbox.InterfaceTypeWIFI
                nif.name.startsWith("rmnet") || nif.name.startsWith("ccmni") || nif.name.startsWith("pdp") -> Libbox.InterfaceTypeCellular
                nif.name.startsWith("eth") -> Libbox.InterfaceTypeEthernet
                else -> Libbox.InterfaceTypeOther
            }
            netInterface.type = type

            // Addresses
            val addrStrings = mutableListOf<String>()
            for (ia in nif.interfaceAddresses) {
                val addr = ia.address
                val prefix = ia.networkPrefixLength.toInt().takeIf { it > 0 } ?: if (addr is Inet4Address) 32 else 128
                val host = addr.hostAddress?.substringBefore('%') ?: continue
                addrStrings.add("$host/$prefix")
            }
            netInterface.addresses = SimpleStringIterator(addrStrings)

            // DNS & Gateway & Metered
            val (dnsList, gatewayList) = vpnService.getDnsAndGatewaysForInterface(nif.name)
            netInterface.dnsServer = SimpleStringIterator(dnsList)
            netInterface.gateway = SimpleStringIterator(gatewayList)
            netInterface.metered = vpnService.isNetworkMetered(nif.name)

            resultList.add(netInterface)
        }

        return object : NetworkInterfaceIterator {
            private var index = 0
            override fun hasNext(): Boolean = index < resultList.size
            override fun next(): io.nekohasekai.libbox.NetworkInterface? {
                return if (hasNext()) resultList[index++] else null
            }
        }
    }

    override fun underNetworkExtension(): Boolean = false
    override fun includeAllNetworks(): Boolean = false
    override fun readWIFIState(): WIFIState? = null
    override fun clearDNSCache() {}
    override fun sendNotification(notification: Notification) {}
    override fun cancelNotification(identifier: String, typeID: Int) {}
    override fun startNeighborMonitor(listener: NeighborUpdateListener) {}
    override fun closeNeighborMonitor(listener: NeighborUpdateListener) {}
    override fun registerMyInterface(name: String) {
        SanitizedLogger.log("Tunnel interface registered: $name")
    }
    override fun usePlatformShell(): Boolean = false
    override fun checkPlatformShell() {}
    override fun openShellSession(
        user: PlatformUser?,
        command: String,
        environ: StringIterator?,
        term: String,
        rows: Int,
        cols: Int
    ): ShellSession? = null
    override fun lookupUser(username: String): PlatformUser? = null
    override fun lookupSFTPServer(): String? = null
    override fun readSystemSSHHostKey(): String? = null
    override fun tailscaleHostname(): String = ""
    override fun usePlatformBridge(): Boolean = false
    override fun createBridge(options: BridgeOptions?): BridgeSession? = null
    override fun localDNSTransport(): LocalDNSTransport? = null

    // CommandServerHandler implementation
    override fun serviceStop() {
        vpnService.stopSelf()
    }
    override fun serviceReload() {}
    override fun getSystemProxyStatus(): SystemProxyStatus? = null
    override fun setSystemProxyEnabled(enabled: Boolean) {}
    override fun triggerNativeCrash() {}
    override fun writeDebugMessage(message: String) {
        SanitizedLogger.log(message, "DEBUG")
    }
    override fun connectSSHAgent(): Int = -1
}
