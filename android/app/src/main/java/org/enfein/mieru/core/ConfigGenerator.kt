package org.enfein.mieru.core

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.enfein.mieru.model.Profile
import org.json.JSONArray
import org.json.JSONObject
import java.net.Inet4Address
import java.net.InetAddress
import java.net.UnknownHostException

object ConfigGenerator {

    suspend fun resolveServerAddress(server: String): String = withContext(Dispatchers.IO) {
        val trimmed = server.trim()
        if (isNumericIpv4(trimmed) || isNumericIpv6(trimmed)) {
            return@withContext trimmed
        }

        try {
            val addresses = InetAddress.getAllByName(trimmed)
            if (addresses.isEmpty()) {
                throw UnknownHostException("Unable to resolve hostname: $trimmed")
            }
            // Prefer IPv4
            val ipv4 = addresses.firstOrNull { it is Inet4Address }
            val chosen = ipv4 ?: addresses.first()
            chosen.hostAddress ?: throw UnknownHostException("Empty host address for $trimmed")
        } catch (e: Exception) {
            throw UnknownHostException("DNS resolution failed for '$trimmed': ${e.message}")
        }
    }

    fun generateCoreConfig(profile: Profile, resolvedIp: String): String {
        val root = JSONObject()

        // 1. Log
        val logObj = JSONObject().apply {
            put("level", "info")
        }
        root.put("log", logObj)

        // 2. DNS
        val dnsServer = JSONObject().apply {
            put("type", "udp")
            put("tag", "remote-dns")
            put("server", "1.1.1.1")
            put("server_port", 53)
            put("detour", "mieru-out")
        }
        val dnsObj = JSONObject().apply {
            put("servers", JSONArray().apply { put(dnsServer) })
            put("final", "remote-dns")
            put("strategy", "ipv4_only")
        }
        root.put("dns", dnsObj)

        // 3. Inbounds (TUN with hijack DNS)
        val excludeCidr = if (resolvedIp.contains(":")) {
            "$resolvedIp/128"
        } else {
            "$resolvedIp/32"
        }

        val tunInbound = JSONObject().apply {
            put("type", "tun")
            put("tag", "tun-in")
            put("address", JSONArray().apply {
                put("172.19.0.1/30")
                put("fdfe:dcba:9876::1/126")
            })
            put("dns_mode", "hijack")
            put("auto_route", true)
            put("strict_route", false)
            put("stack", "gvisor")
            put("route_exclude_address", JSONArray().apply { put(excludeCidr) })
        }
        root.put("inbounds", JSONArray().apply { put(tunInbound) })

        // 4. Outbounds (mieru)
        val mieruOutbound = JSONObject().apply {
            put("type", "mieru")
            put("tag", "mieru-out")
            put("server", resolvedIp)
            put("server_port", profile.serverPort)
            put("transport", profile.transport)
            put("username", profile.username)
            put("password", profile.password)
            if (!profile.trafficPattern.isNullOrBlank()) {
                put("traffic_pattern", profile.trafficPattern)
            }
        }
        root.put("outbounds", JSONArray().apply { put(mieruOutbound) })

        // 5. Route (with hijack-dns rule on port 53)
        val dnsHijackRule = JSONObject().apply {
            put("port", 53)
            put("action", "hijack-dns")
        }
        val routeObj = JSONObject().apply {
            put("rules", JSONArray().apply { put(dnsHijackRule) })
            put("final", "mieru-out")
            put("auto_detect_interface", true)
        }
        root.put("route", routeObj)

        return root.toString(2)
    }

    private fun isNumericIpv4(ip: String): Boolean {
        val parts = ip.split(".")
        if (parts.size != 4) return false
        return parts.all { part ->
            part.toIntOrNull()?.let { it in 0..255 } == true
        }
    }

    private fun isNumericIpv6(ip: String): Boolean {
        return ip.contains(":")
    }
}
